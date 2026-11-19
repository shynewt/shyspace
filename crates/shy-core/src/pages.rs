//! Pages are markdown held in a single Yjs text, so the desktop editor (Yjs)
//! and agents (yrs) edit the same structure natively.

use anyhow::{Context, Result, ensure};
use yrs::{Doc, GetString, ReadTxn, StateVector, Text, Transact, Update, updates::decoder::Decode};

const CONTENT: &str = "content";

fn load(state: Option<&[u8]>) -> Result<Doc> {
    let doc = Doc::new();
    if let Some(state) = state {
        doc.transact_mut().apply_update(Update::decode_v1(state)?)?;
    }
    Ok(doc)
}

fn encode(doc: &Doc) -> Vec<u8> {
    doc.transact().encode_state_as_update_v1(&StateVector::default())
}

fn text(doc: &Doc) -> String {
    doc.get_or_insert_text(CONTENT).get_string(&doc.transact())
}

/// Applies a Yjs update to a page's state and returns the new state.
pub fn merge(state: Option<&[u8]>, update: &[u8]) -> Result<Vec<u8>> {
    let doc = load(state)?;
    doc.transact_mut().apply_update(Update::decode_v1(update)?)?;
    Ok(encode(&doc))
}

pub fn read(state: Option<&[u8]>) -> Result<String> {
    Ok(text(&load(state)?))
}

/// Edits made on behalf of agents. Each returns the Yjs update to broadcast.
pub enum Edit<'a> {
    Append(&'a str),
    Replace { find: &'a str, with: &'a str },
}

pub fn edit(state: Option<&[u8]>, edit: Edit) -> Result<Vec<u8>> {
    let doc = load(state)?;
    let content = doc.get_or_insert_text(CONTENT);
    let current = content.get_string(&doc.transact());
    let mut txn = doc.transact_mut();
    match edit {
        Edit::Append(addition) => {
            let separator = if current.is_empty() || current.ends_with("\n\n") {
                ""
            } else if current.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            content.insert(&mut txn, current.len() as u32, &format!("{separator}{addition}"));
        }
        Edit::Replace { find, with } => {
            ensure!(!find.is_empty(), "text to find must not be empty");
            let start = current.find(find).context("text to replace was not found")?;
            let next_char = current[start..].chars().next().map_or(1, char::len_utf8);
            ensure!(
                !current[start + next_char..].contains(find),
                "text to replace appears more than once; include more context"
            );
            // Touch only the part that differs, so concurrent edits next to the
            // passage keep their place instead of racing a full re-insert.
            let (prefix, suffix) = common_affixes(find, with);
            let removed = &find[prefix..find.len() - suffix];
            let inserted = &with[prefix..with.len() - suffix];
            let at = (start + prefix) as u32;
            content.remove_range(&mut txn, at, removed.len() as u32);
            content.insert(&mut txn, at, inserted);
        }
    }
    Ok(txn.encode_update_v1())
}

/// Byte lengths of the shared prefix and suffix, on char boundaries and
/// without overlapping.
fn common_affixes(old: &str, new: &str) -> (usize, usize) {
    let prefix: usize = old
        .chars()
        .zip(new.chars())
        .take_while(|(left, right)| left == right)
        .map(|(left, _)| left.len_utf8())
        .sum();
    let suffix: usize = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(left, _)| left.len_utf8())
        .sum();
    (prefix, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_edits_converge() {
        let base = merge(None, &edit(None, Edit::Append("# Plan\n\nShip it.")).unwrap()).unwrap();
        let ours = edit(Some(&base), Edit::Append("- step one")).unwrap();
        let theirs = edit(
            Some(&base),
            Edit::Replace {
                find: "Ship it.",
                with: "Ship it soon.",
            },
        )
        .unwrap();

        let left = merge(Some(&merge(Some(&base), &ours).unwrap()), &theirs).unwrap();
        let right = merge(Some(&merge(Some(&base), &theirs).unwrap()), &ours).unwrap();
        assert_eq!(read(Some(&left)).unwrap(), read(Some(&right)).unwrap());
        assert_eq!(
            read(Some(&left)).unwrap(),
            "# Plan\n\nShip it soon.\n\n- step one"
        );
    }

    #[test]
    fn replace_requires_a_unique_match() {
        let state = merge(None, &edit(None, Edit::Append("a b a")).unwrap()).unwrap();
        assert!(edit(Some(&state), Edit::Replace { find: "a", with: "c" }).is_err());
        assert!(edit(Some(&state), Edit::Replace { find: "z", with: "c" }).is_err());
    }

    #[test]
    fn affixes_respect_char_boundaries() {
        assert_eq!(common_affixes("Ship it.", "Ship it soon."), (7, 1));
        assert_eq!(common_affixes("añb", "aéb"), (1, 1));
        assert_eq!(common_affixes("aa", "aaa"), (2, 0));
    }
}
