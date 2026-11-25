//! Pages are markdown held in a single Yjs text, so the desktop editor (Yjs)
//! and agents (yrs) edit the same structure natively.

pub mod cursors;
mod update_check;

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;
use yrs::{
    Assoc, ClientID, Doc, GetString, ID, ReadTxn, StateVector, StickyIndex, Text, Transact, Update,
    updates::{decoder::Decode, encoder::Encode},
};

use update_check::{ClientEnds, MAX_CLIENT_ID, check_update, inserted_texts};

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

/// A merge couldn't rebuild a page from updates that applied before, so it
/// no longer holds all of them. Replaying is deterministic, so this
/// shouldn't happen; it isn't the fault of the update being added.
#[derive(Debug)]
pub struct MergeLost;

impl std::fmt::Display for MergeLost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a page merge lost updates that applied before")
    }
}

/// Merges many updates to one page in memory, so replaying a page's history
/// costs the size of each update rather than the whole page every time.
#[derive(Default)]
pub struct PageMerge {
    base: Option<Vec<u8>>,
    doc: Doc,
    ends: ClientEnds,
    /// The updates added on top of `base`, with their clients.
    accepted: Vec<(Vec<u8>, u64)>,
}

impl PageMerge {
    /// Starts from a page's stored state.
    pub fn from_state(state: Option<&[u8]>) -> Result<Self> {
        let ends = match state {
            Some(state) => ClientEnds::of_state(state).context("reading the page state")?,
            None => ClientEnds::default(),
        };
        Ok(Self {
            base: state.map(<[u8]>::to_vec),
            doc: load(state)?,
            ends,
            accepted: Vec::new(),
        })
    }

    /// Takes and refuses the same updates as [`merge`] would, one after
    /// another; a refused one leaves the page as it was.
    pub fn add(&mut self, update: &[u8], client: u64) -> Result<()> {
        let new_ends = check_update(update, &self.ends, client)?;
        let decoded = Update::decode_v1(update)?;
        let applied = self.doc.transact_mut().apply_update(decoded);
        if let Err(error) = applied {
            self.replay().context(MergeLost)?;
            return Err(error.into());
        }
        self.ends.extend(new_ends);
        self.accepted.push((update.to_vec(), client));
        Ok(())
    }

    /// Takes back the last update added, as if it never came.
    pub fn undo_last(&mut self) -> Result<()> {
        self.accepted.pop().context("no page update to take back")?;
        self.replay().context(MergeLost)
    }

    /// Rebuilds the page from its base and the updates still accepted.
    fn replay(&mut self) -> Result<()> {
        let accepted = std::mem::take(&mut self.accepted);
        let fresh = Self::from_state(self.base.as_deref())?;
        *self = fresh;
        for (update, client) in accepted {
            self.add(&update, client)
                .context("replaying a page update that applied before")?;
        }
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

/// Text a page update inserts, with the Yjs id of its first character.
pub struct InsertedText<'a> {
    pub client: u64,
    /// Counts UTF-16 units, as Yjs does.
    pub clock: u32,
    pub text: &'a str,
}

/// The pieces of text `update` inserts. It must be an update [`merge`]
/// accepted.
pub fn insertions(update: &[u8]) -> Result<Vec<InsertedText<'_>>> {
    inserted_texts(update)
}

/// A spot in a page that stays with the character `byte` of `inserted`
/// starts, as an encoded Yjs relative position.
pub fn position_in(inserted: &InsertedText, byte: usize) -> Result<Vec<u8>> {
    let before = inserted
        .text
        .get(..byte)
        .context("a page position inside a character or past its text")?;
    let offset = u32::try_from(before.encode_utf16().count()).context("text too long")?;
    let clock = inserted
        .clock
        .checked_add(offset)
        .context("clock past what yrs holds")?;
    Ok(StickyIndex::from_id(ID::new(ClientID::new(inserted.client), clock), Assoc::After).encode_v1())
}

/// Which of `positions` (from [`position_in`]) name text the page holds,
/// even if deleted since. One whose text never arrived, or was undone with
/// a removed member's late edits, names nothing.
pub fn positions_held(state: Option<&[u8]>, positions: &[&[u8]]) -> Result<Vec<bool>> {
    let doc = load(state)?;
    let txn = doc.transact();
    positions
        .iter()
        .map(|position| {
            let position = StickyIndex::decode_v1(position).context("reading a page position")?;
            Ok(position.get_offset(&txn).is_some())
        })
        .collect()
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
    fn a_position_in_inserted_text_stays_held_after_its_text_is_deleted() {
        let first = edit(None, Edit::Append("# Plan\n\nAsk 🚀 Ada today"), 1).unwrap();
        let page = merge(None, &first, 1).unwrap();
        let inserted = insertions(&first).unwrap();
        assert_eq!(inserted.len(), 1);
        let ada = inserted[0].text.find("Ada").unwrap();
        let position = position_in(&inserted[0], ada).unwrap();
        let removal = edit(
            Some(&page),
            Edit::Replace {
                find: "Ask 🚀 Ada today",
                with: "Done",
            },
            3,
        )
        .unwrap();
        let page = merge(Some(&page), &removal, 3).unwrap();

        assert_eq!(positions_held(Some(&page), &[&position]).unwrap(), [true]);
        assert_eq!(positions_held(None, &[&position]).unwrap(), [false]);
    }

    #[test]
    fn a_position_must_fall_between_characters() {
        let update = edit(None, Edit::Append("é"), 1).unwrap();
        let inserted = insertions(&update).unwrap();
        assert!(position_in(&inserted[0], 1).is_err());
        assert!(position_in(&inserted[0], 2).is_ok());
    }

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
