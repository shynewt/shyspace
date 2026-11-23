//! Pages are markdown held in a single Yjs text, so the desktop editor (Yjs)
//! and agents (yrs) edit the same structure natively.

mod update_check;

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;
use yrs::{Doc, GetString, ReadTxn, StateVector, Text, Transact, Update, updates::decoder::Decode};

use update_check::{ClientEnds, MAX_CLIENT_ID, check_update};

const CONTENT: &str = "content";

const CLIENT_ID_CONTEXT: &str = "shyspace page client id v1";
pub const CLIENT_ID_NONCE_BYTES: usize = 16;

/// The Yjs client id `author` writes `page` with in an editing session that
/// started with `nonce`. Every device can work it out from the op, so a
/// member can't write under someone else's id: finding a nonce that lands on
/// a given id takes about 2^53 tries.
pub fn client_id(author: &PublicKey, page: &str, nonce: &[u8; CLIENT_ID_NONCE_BYTES]) -> u64 {
    // The author and nonce have fixed lengths, so the page between them
    // can't shift bytes from one into the other.
    let mut hasher = blake3::Hasher::new_derive_key(CLIENT_ID_CONTEXT);
    hasher.update(author.as_bytes());
    hasher.update(page.as_bytes());
    hasher.update(nonce);
    let hash = hasher.finalize();
    let mut first_bytes = [0; 8];
    first_bytes.copy_from_slice(&hash.as_bytes()[..8]);
    u64::from_le_bytes(first_bytes) & MAX_CLIENT_ID
}

fn load(state: Option<&[u8]>) -> Result<Doc> {
    load_into(Doc::new(), state)
}

fn load_into(doc: Doc, state: Option<&[u8]>) -> Result<Doc> {
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

/// Applies a Yjs update from anyone to a page's state and returns the new
/// state. The update may only add pieces for Yjs client `client`, its
/// author's [`client_id`].
pub fn merge(state: Option<&[u8]>, update: &[u8], client: u64) -> Result<Vec<u8>> {
    let ends = match state {
        Some(state) => ClientEnds::of_state(state).context("reading the page state")?,
        None => ClientEnds::default(),
    };
    check_update(update, &ends, client)?;
    let doc = load(state)?;
    doc.transact_mut().apply_update(Update::decode_v1(update)?)?;
    Ok(encode(&doc))
}

/// Merges many updates to one page in memory, so replaying a page's history
/// costs the size of each update rather than the whole page every time.
#[derive(Default)]
pub struct PageMerge {
    doc: Doc,
    ends: ClientEnds,
    accepted: Vec<Vec<u8>>,
}

impl PageMerge {
    /// Takes and refuses the same updates as [`merge`] would, one after
    /// another; a refused one leaves the page as it was.
    pub fn add(&mut self, update: &[u8], client: u64) -> Result<()> {
        let new_ends = check_update(update, &self.ends, client)?;
        let decoded = Update::decode_v1(update)?;
        let applied = self.doc.transact_mut().apply_update(decoded);
        if let Err(error) = applied {
            self.doc = Doc::new();
            for accepted in &self.accepted {
                self.doc
                    .transact_mut()
                    .apply_update(Update::decode_v1(accepted)?)?;
            }
            return Err(error.into());
        }
        self.ends.extend(new_ends);
        self.accepted.push(update.to_vec());
        Ok(())
    }

    pub fn state(&self) -> Vec<u8> {
        encode(&self.doc)
    }

    pub fn is_empty(&self) -> bool {
        self.accepted.is_empty()
    }
}

pub fn read(state: Option<&[u8]>) -> Result<String> {
    Ok(text(&load(state)?))
}

/// Edits made on behalf of agents. Each returns the Yjs update to broadcast.
pub enum Edit<'a> {
    Append(&'a str),
    Replace { find: &'a str, with: &'a str },
}

/// Makes the update for `edit`, written as Yjs client `client`.
pub fn edit(state: Option<&[u8]>, edit: Edit, client: u64) -> Result<Vec<u8>> {
    let doc = load_into(Doc::with_client_id(client), state)?;
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
        let base = merge(
            None,
            &edit(None, Edit::Append("# Plan\n\nShip it."), 1).unwrap(),
            1,
        )
        .unwrap();
        let ours = edit(Some(&base), Edit::Append("- step one"), 2).unwrap();
        let theirs = edit(
            Some(&base),
            Edit::Replace {
                find: "Ship it.",
                with: "Ship it soon.",
            },
            3,
        )
        .unwrap();

        let left = merge(Some(&merge(Some(&base), &ours, 2).unwrap()), &theirs, 3).unwrap();
        let right = merge(Some(&merge(Some(&base), &theirs, 3).unwrap()), &ours, 2).unwrap();
        assert_eq!(read(Some(&left)).unwrap(), read(Some(&right)).unwrap());
        assert_eq!(
            read(Some(&left)).unwrap(),
            "# Plan\n\nShip it soon.\n\n- step one"
        );
    }

    #[test]
    fn replace_requires_a_unique_match() {
        let state = merge(None, &edit(None, Edit::Append("a b a"), 1).unwrap(), 1).unwrap();
        assert!(edit(Some(&state), Edit::Replace { find: "a", with: "c" }, 1).is_err());
        assert!(edit(Some(&state), Edit::Replace { find: "z", with: "c" }, 1).is_err());
    }

    #[test]
    fn client_ids_depend_on_author_page_and_nonce() {
        let alice = iroh::SecretKey::from_bytes(&[1; 32]).public();
        let bob = iroh::SecretKey::from_bytes(&[2; 32]).public();
        let nonce = [7; CLIENT_ID_NONCE_BYTES];
        let id = client_id(&alice, "page", &nonce);
        assert_eq!(id, client_id(&alice, "page", &nonce));
        assert!(id <= MAX_CLIENT_ID);
        assert_ne!(id, client_id(&bob, "page", &nonce));
        assert_ne!(id, client_id(&alice, "other page", &nonce));
        assert_ne!(id, client_id(&alice, "page", &[8; CLIENT_ID_NONCE_BYTES]));
    }

    #[test]
    fn a_bound_update_adds_pieces_only_for_its_own_client() {
        let first = edit(None, Edit::Append("Hello world"), 5).unwrap();
        assert!(merge(None, &first, 6).is_err());
        let state = merge(None, &first, 5).unwrap();

        // Another editor may delete the first one's text and write next to it.
        let reply = edit(
            Some(&state),
            Edit::Replace {
                find: "world",
                with: "there",
            },
            6,
        )
        .unwrap();
        let state = merge(Some(&state), &reply, 6).unwrap();
        assert_eq!(read(Some(&state)).unwrap(), "Hello there");

        let deletion_only = edit(
            Some(&state),
            Edit::Replace {
                find: "Hello ",
                with: "",
            },
            7,
        )
        .unwrap();
        let state = merge(Some(&state), &deletion_only, 7).unwrap();
        assert_eq!(read(Some(&state)).unwrap(), "there");
    }

    #[test]
    fn a_bound_update_cannot_carry_two_clients() {
        let first = edit(None, Edit::Append("one"), 5).unwrap();
        let state = merge(None, &first, 5).unwrap();
        let second = edit(Some(&state), Edit::Append("two"), 6).unwrap();
        let both = yrs::merge_updates_v1([first.as_slice(), second.as_slice()]).unwrap();
        assert!(merge(None, &both, 5).is_err());
        assert!(merge(None, &both, 6).is_err());
    }

    #[test]
    fn affixes_respect_char_boundaries() {
        assert_eq!(common_affixes("Ship it.", "Ship it soon."), (7, 1));
        assert_eq!(common_affixes("añb", "aéb"), (1, 1));
        assert_eq!(common_affixes("aa", "aaa"), (2, 0));
    }
}
