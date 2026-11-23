//! Edits, renames and deletes. Each is kept apart from what it changes, so it
//! applies the same whether it arrives before or after its target. Deletes are
//! permanent markers that win over everything else about their target.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::{
    markdown::visible_text,
    ops::{Op, is_valid_channel_name, rule_violation},
    views::Change,
};

/// What an id names in this workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Message,
    Channel,
    Page,
    File,
}

impl ItemKind {
    pub fn noun(self) -> &'static str {
        match self {
            ItemKind::Message => "message",
            ItemKind::Channel => "channel",
            ItemKind::Page => "page",
            ItemKind::File => "file",
        }
    }
}

impl Store {
    pub(crate) fn item_kind(&self, id: &str) -> Result<Option<ItemKind>> {
        let kind: Option<String> = self
            .db
            .query_row(
                "SELECT 'message' FROM messages WHERE id = ?1
                 UNION ALL SELECT 'channel' FROM channels WHERE id = ?1
                 UNION ALL SELECT 'page' FROM pages WHERE id = ?1 AND created_ms IS NOT NULL
                 UNION ALL SELECT 'file' FROM files WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(kind.map(|kind| match kind.as_str() {
            "message" => ItemKind::Message,
            "channel" => ItemKind::Channel,
            "file" => ItemKind::File,
            _ => ItemKind::Page,
        }))
    }

    pub(crate) fn is_deleted(&self, id: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT 1 FROM deleted WHERE id = ?", [id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Whether `op` can't change anything: its target was deleted, or it
    /// re-sends a message id that already exists. Only its author can use an
    /// id, and the one earliest in their log wins (private content can open
    /// out of order), so every member keeps the same one. A message posted
    /// into a deleted channel is marked deleted itself, so it can't later
    /// reappear elsewhere.
    pub(super) fn has_no_effect(&self, op: &Op, author: &[u8], seq: u64) -> Result<bool> {
        if let Op::Message { id, channel, .. } = op {
            let is_earlier_kept = self.message_seq(id)?.is_some_and(|kept| kept <= seq);
            if self.is_deleted(id)? || is_earlier_kept {
                return Ok(true);
            }
            if self.is_deleted(channel)? {
                self.mark_deleted(id)?;
                return Ok(true);
            }
        }
        if let Op::File { file } = op {
            if self.file_seq(&file.id)?.is_some_and(|kept| kept <= seq) {
                return Ok(true);
            }
            if self.is_deleted(&file.id)? {
                self.remove_late_file(&file.id, author, &file.blob.id)?;
                return Ok(true);
            }
            let homes = [Some(&file.place), file.message.as_ref()];
            for home in homes.into_iter().flatten() {
                if self.is_deleted(home)? {
                    self.mark_deleted(&file.id)?;
                    self.remove_late_file(home, author, &file.blob.id)?;
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        let target = match op {
            Op::ChannelCreate { id, .. }
            | Op::ChannelRename { id, .. }
            | Op::Message { id, .. }
            | Op::MessageEdit { id, .. }
            | Op::FileRename { id, .. }
            | Op::PageCreate { id, .. }
            | Op::PageRename { id, .. } => id,
            Op::Reaction { message, .. } => message,
            Op::DocUpdate { page, .. } => page,
            Op::Profile { .. }
            | Op::StorageSettings { .. }
            | Op::Delete { .. }
            | Op::Status { .. }
            | Op::SealingKey { .. }
            | Op::KeyGrant { .. }
            | Op::Sealed { .. }
            | Op::ProfileDetails { .. }
            | Op::ProfilePicture { .. }
            | Op::File { .. } => {
                return Ok(false);
            }
        };
        self.is_deleted(target)
    }

    /// A message author's edits replace one another in their own order.
    pub(super) fn apply_message_edit(&self, id: &str, text: &str, lamport: u64) -> Result<Option<Change>> {
        let changed = self.db.execute(
            "INSERT INTO message_edits (message, text, lamport) VALUES (?1, ?2, ?3)
             ON CONFLICT (message) DO UPDATE SET text = ?2, lamport = ?3 WHERE lamport < ?3",
            params![
                id,
                crate::ops::cut_to(text, crate::ops::MAX_MESSAGE_CHARS),
                lamport
            ],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        let Some(channel) = self.message_channel(id)? else {
            // The message hasn't arrived yet; it picks the edit up when it does.
            return Ok(None);
        };
        self.reindex_message(id)?;
        Ok(Some(Change::Messages { channel }))
    }

    pub(super) fn apply_channel_rename(
        &self,
        id: &str,
        name: &str,
        lamport: u64,
        author: &[u8],
    ) -> Result<Change> {
        if !is_valid_channel_name(name) {
            return Err(rule_violation("invalid channel name"));
        }
        self.db.execute(
            "INSERT INTO channel_names (channel, name, lamport, author) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (channel) DO UPDATE SET name = ?2, lamport = ?3, author = ?4
             WHERE lamport < ?3 OR (lamport = ?3 AND author < ?4)",
            params![id, name, lamport, author],
        )?;
        Ok(Change::Channels)
    }

    /// Removes whatever `id` names. Ids are random, so in practice it's one
    /// thing; purging every kind keeps replicas in step whatever it is.
    /// `deleter` may remove the deleted files' blobs from the bucket.
    pub(super) fn apply_delete(&self, id: &str, deleter: &[u8]) -> Result<Option<Change>> {
        self.db.execute(
            "INSERT OR IGNORE INTO deleted (id, deleter) VALUES (?, ?)",
            params![id, deleter],
        )?;
        let kind = self.item_kind(id)?;
        let message_channel = self.message_channel(id)?;
        let file_changes = self.purge_files_of(id, deleter)?;
        self.purge_message(id)?;
        self.purge_channel(id)?;
        self.db.execute("DELETE FROM pages WHERE id = ?", [id])?;
        self.remove_from_search("page", id)?;
        Ok(match kind {
            Some(ItemKind::Channel) => Some(Change::Channels),
            Some(ItemKind::Page) => Some(Change::Pages),
            Some(ItemKind::Message) => message_channel.map(|channel| Change::Messages { channel }),
            Some(ItemKind::File) => file_changes.into_iter().next(),
            None => None,
        })
    }

    pub(super) fn reindex_message(&self, id: &str) -> Result<()> {
        let text: String = self.db.query_row(
            "SELECT COALESCE(e.text, m.text) FROM messages m
             LEFT JOIN message_edits e ON e.message = m.id WHERE m.id = ?",
            [id],
            |row| row.get(0),
        )?;
        self.index_for_search("message", id, "", &visible_text(&text))
    }

    /// A file that arrives after `deleted` (it or its home) was deleted: its
    /// pieces leave the bucket through whoever deleted it, as if it had been
    /// there then.
    fn remove_late_file(&self, deleted: &str, author: &[u8], blob_id: &str) -> Result<()> {
        let deleter: Option<Vec<u8>> = self
            .db
            .query_row("SELECT deleter FROM deleted WHERE id = ?", [deleted], |row| {
                row.get(0)
            })
            .optional()?
            .flatten();
        if let Some(deleter) = deleter {
            self.request_blob_removal(author, blob_id, &deleter)?;
        }
        Ok(())
    }

    pub(super) fn mark_deleted(&self, id: &str) -> Result<()> {
        self.db
            .execute("INSERT OR IGNORE INTO deleted (id) VALUES (?)", [id])?;
        Ok(())
    }

    pub(crate) fn message_channel(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT channel FROM messages WHERE id = ?", [id], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Where the kept message with this id sits in its author's log (0 for
    /// messages stored before that was recorded).
    fn message_seq(&self, id: &str) -> Result<Option<u64>> {
        Ok(self
            .db
            .query_row(
                "SELECT COALESCE(seq, 0) FROM messages WHERE id = ?",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Its messages are marked deleted too, so no later op revives them.
    fn purge_channel(&self, id: &str) -> Result<()> {
        let messages: Vec<String> = self
            .db
            .prepare("SELECT id FROM messages WHERE channel = ?")?
            .query_map([id], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        for message in messages {
            self.mark_deleted(&message)?;
            self.purge_message(&message)?;
        }
        self.db.execute("DELETE FROM channels WHERE id = ?", [id])?;
        self.db
            .execute("DELETE FROM channel_names WHERE channel = ?", [id])?;
        Ok(())
    }

    fn purge_message(&self, id: &str) -> Result<()> {
        self.db.execute("DELETE FROM messages WHERE id = ?", [id])?;
        self.db
            .execute("DELETE FROM message_edits WHERE message = ?", [id])?;
        self.db.execute("DELETE FROM reactions WHERE message = ?", [id])?;
        self.remove_from_search("message", id)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use iroh::{PublicKey, SecretKey};

    use super::*;
    use crate::{
        ops::{OpBody, VerifiedOp, new_owned_id},
        storage::TEST_DB_KEY,
    };

    /// Everything a member could see.
    type View = (
        Vec<(String, String)>,
        Vec<(String, String, bool, usize)>,
        Vec<(String, Option<String>)>,
        Vec<String>,
    );

    /// Ops from several authors. Each author's ops arrive in their order
    /// (logs are gap-free); across authors, any order is possible.
    struct Scenario {
        ops: Vec<VerifiedOp>,
        channels: Vec<String>,
    }

    impl Scenario {
        fn new() -> Self {
            Self {
                ops: Vec::new(),
                channels: Vec::new(),
            }
        }

        fn add(&mut self, author: &PublicKey, lamport: u64, op: Op) {
            if let Op::ChannelCreate { id, .. } = &op {
                self.channels.push(id.clone());
            }
            let seq = self.ops.iter().filter(|op| op.author == *author).count() as u64 + 1;
            self.ops.push(VerifiedOp {
                author: *author,
                seq,
                epoch: 1,
                body: OpBody {
                    lamport,
                    timestamp_ms: lamport as i64,
                    op,
                },
            });
        }

        fn view_after(&self, order: &[usize]) -> View {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
            for _ in 0..2 {
                for &index in order {
                    store.apply(&self.ops[index]).unwrap();
                }
            }
            let reader = self.ops[0].author;
            let channels = store
                .channels()
                .unwrap()
                .into_iter()
                .map(|c| (c.id, c.name))
                .collect();
            let messages = self
                .channels
                .iter()
                .flat_map(|channel| store.messages(channel, &reader, None, 50).unwrap())
                .map(|m| (m.channel, m.text, m.is_edited, m.reactions.len()))
                .collect();
            let pages = store
                .pages()
                .unwrap()
                .into_iter()
                .map(|p| (p.id, p.parent))
                .collect();
            let hits = store
                .search("hello", &crate::storage::SearchFilters::default(), 10)
                .unwrap()
                .into_iter()
                .map(|hit| hit.id)
                .collect();
            (channels, messages, pages, hits)
        }

        /// The view is the same for every possible arrival order, applied twice.
        fn assert_converges(&self) -> View {
            let orders: Vec<_> = permutations(&(0..self.ops.len()).collect::<Vec<_>>())
                .into_iter()
                .filter(|order| self.keeps_each_authors_order(order))
                .collect();
            let expected = self.view_after(&orders[0]);
            for order in &orders[1..] {
                assert_eq!(self.view_after(order), expected, "order {order:?}");
            }
            expected
        }

        /// Files and their names, the same for every arrival order, applied twice.
        fn assert_files_converge(&self) -> Vec<(String, String)> {
            let files_after = |order: &[usize]| {
                let dir = tempfile::tempdir().unwrap();
                let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
                for _ in 0..2 {
                    for &index in order {
                        store.apply(&self.ops[index]).unwrap();
                    }
                }
                let mut files: Vec<_> = store
                    .all_files()
                    .unwrap()
                    .into_iter()
                    .map(|file| (file.id, file.name))
                    .collect();
                files.sort();
                files
            };
            let orders: Vec<_> = permutations(&(0..self.ops.len()).collect::<Vec<_>>())
                .into_iter()
                .filter(|order| self.keeps_each_authors_order(order))
                .collect();
            let expected = files_after(&orders[0]);
            for order in &orders[1..] {
                assert_eq!(files_after(order), expected, "order {order:?}");
            }
            expected
        }

        fn keeps_each_authors_order(&self, order: &[usize]) -> bool {
            order.iter().enumerate().all(|(position, &later)| {
                order[..position].iter().all(|&earlier| {
                    let (earlier, later) = (&self.ops[earlier], &self.ops[later]);
                    earlier.author != later.author || earlier.seq < later.seq
                })
            })
        }
    }

    fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut all = Vec::new();
        for (position, &first) in items.iter().enumerate() {
            let rest = [&items[..position], &items[position + 1..]].concat();
            for mut tail in permutations(&rest) {
                tail.insert(0, first);
                all.push(tail);
            }
        }
        all
    }

    fn people<const N: usize>() -> [PublicKey; N] {
        std::array::from_fn(|_| SecretKey::generate().public())
    }

    fn message(id: &str, channel: &str, text: &str) -> Op {
        Op::Message {
            id: id.into(),
            channel: channel.into(),
            text: text.into(),
            reply_to: None,
        }
    }

    #[test]
    fn a_deleted_message_stays_deleted_in_any_order() {
        let [ada, bob, cat] = people();
        let mut scenario = Scenario::new();
        let channel = new_owned_id(&ada);
        let bobs = new_owned_id(&bob);
        scenario.add(
            &ada,
            1,
            Op::ChannelCreate {
                id: channel.clone(),
                name: "general".into(),
            },
        );
        scenario.add(&bob, 2, message(&bobs, &channel, "hello"));
        scenario.add(
            &bob,
            3,
            Op::MessageEdit {
                id: bobs.clone(),
                text: "hello again".into(),
            },
        );
        scenario.add(
            &cat,
            4,
            Op::Reaction {
                message: bobs.clone(),
                emoji: "👍".into(),
                active: true,
            },
        );
        scenario.add(&ada, 5, Op::Delete { id: bobs });

        let (channels, messages, _, hits) = scenario.assert_converges();
        assert_eq!(channels.len(), 1);
        assert!(messages.is_empty() && hits.is_empty());
    }

    #[test]
    fn edits_reach_everyone_in_their_order() {
        let [ada, bob] = people();
        let mut scenario = Scenario::new();
        let channel = new_owned_id(&ada);
        let bobs = new_owned_id(&bob);
        scenario.add(
            &ada,
            1,
            Op::ChannelCreate {
                id: channel.clone(),
                name: "general".into(),
            },
        );
        scenario.add(&bob, 2, message(&bobs, &channel, "hello"));
        scenario.add(
            &bob,
            5,
            Op::MessageEdit {
                id: bobs.clone(),
                text: "hello there".into(),
            },
        );
        scenario.add(
            &ada,
            6,
            Op::Reaction {
                message: bobs.clone(),
                emoji: "👀".into(),
                active: true,
            },
        );
        scenario.add(
            &bob,
            7,
            Op::MessageEdit {
                id: bobs.clone(),
                text: "hello, final".into(),
            },
        );

        let (_, messages, _, hits) = scenario.assert_converges();
        assert_eq!(messages, vec![(channel, "hello, final".to_string(), true, 1)]);
        assert_eq!(hits, vec![bobs]);
    }

    #[test]
    fn the_latest_channel_rename_wins_and_a_delete_beats_it_in_any_order() {
        let [ada, bob, cat] = people();
        let mut scenario = Scenario::new();
        let channel = new_owned_id(&ada);
        scenario.add(
            &ada,
            1,
            Op::ChannelCreate {
                id: channel.clone(),
                name: "random".into(),
            },
        );
        scenario.add(
            &bob,
            3,
            Op::ChannelRename {
                id: channel.clone(),
                name: "fun".into(),
            },
        );
        scenario.add(
            &cat,
            4,
            Op::ChannelRename {
                id: channel.clone(),
                name: "off-topic".into(),
            },
        );
        let (channels, ..) = scenario.assert_converges();
        assert_eq!(channels, vec![(channel.clone(), "off-topic".to_string())]);

        scenario.add(&bob, 5, message(&new_owned_id(&bob), &channel, "hello"));
        scenario.add(&ada, 6, Op::Delete { id: channel });
        let (channels, messages, _, hits) = scenario.assert_converges();
        assert!(channels.is_empty() && messages.is_empty() && hits.is_empty());
    }

    #[test]
    fn a_message_moved_out_of_a_deleted_channel_stays_deleted_in_any_order() {
        let [ada, mallory] = people();
        let mut scenario = Scenario::new();
        let (doomed, kept) = (new_owned_id(&ada), new_owned_id(&ada));
        let moved = new_owned_id(&mallory);
        scenario.add(
            &ada,
            1,
            Op::ChannelCreate {
                id: doomed.clone(),
                name: "doomed".into(),
            },
        );
        scenario.add(
            &ada,
            2,
            Op::ChannelCreate {
                id: kept.clone(),
                name: "kept".into(),
            },
        );
        scenario.add(&mallory, 3, message(&moved, &doomed, "hello"));
        scenario.add(&mallory, 5, message(&moved, &kept, "hello, moved"));
        scenario.add(&ada, 6, Op::Delete { id: doomed });

        let (channels, messages, _, hits) = scenario.assert_converges();
        assert_eq!(channels, vec![(kept, "kept".to_string())]);
        assert!(messages.is_empty() && hits.is_empty());
    }

    #[test]
    fn a_reused_message_id_keeps_its_first_message_in_any_order() {
        let [ada, mallory] = people();
        let mut scenario = Scenario::new();
        let (doomed, kept) = (new_owned_id(&ada), new_owned_id(&ada));
        let reused = new_owned_id(&mallory);
        scenario.add(
            &ada,
            1,
            Op::ChannelCreate {
                id: doomed.clone(),
                name: "doomed".into(),
            },
        );
        scenario.add(
            &ada,
            2,
            Op::ChannelCreate {
                id: kept.clone(),
                name: "kept".into(),
            },
        );
        // The second send has the lower clock, so "earliest clock wins" would pick it.
        scenario.add(&mallory, 5, message(&reused, &doomed, "hello"));
        scenario.add(&mallory, 3, message(&reused, &kept, "hello, moved"));
        scenario.add(&ada, 6, Op::Delete { id: doomed });

        let (_, messages, _, hits) = scenario.assert_converges();
        assert!(messages.is_empty() && hits.is_empty());
    }

    #[test]
    fn a_deleted_pages_children_move_to_the_top_in_any_order() {
        let [ada, bob, cat] = people();
        let mut scenario = Scenario::new();
        let parent = new_owned_id(&ada);
        let child = new_owned_id(&bob);
        scenario.add(
            &ada,
            1,
            Op::PageCreate {
                id: parent.clone(),
                parent: None,
                title: "hello".into(),
            },
        );
        scenario.add(
            &bob,
            2,
            Op::PageCreate {
                id: child.clone(),
                parent: Some(parent.clone()),
                title: "child".into(),
            },
        );
        scenario.add(
            &cat,
            3,
            Op::PageRename {
                id: parent.clone(),
                title: "hello world".into(),
            },
        );
        scenario.add(&ada, 4, Op::Delete { id: parent });

        let (_, _, pages, hits) = scenario.assert_converges();
        assert_eq!(pages, vec![(child, None)]);
        assert!(hits.is_empty());
    }

    fn shared_file(id: &str, channel: &str, name: &str) -> Op {
        let blob = crate::files::blob::seal(&mut &b"bytes"[..], 5, |_, _, _| Ok(())).unwrap();
        Op::File {
            file: crate::files::SharedFile {
                id: id.to_owned(),
                place: channel.to_owned(),
                message: None,
                name: name.to_owned(),
                media_type: "text/plain".into(),
                blob,
            },
        }
    }

    #[test]
    fn the_newest_file_rename_wins_whatever_arrives_first() {
        let [ada, bob, cat] = people();
        let mut scenario = Scenario::new();
        let channel = new_owned_id(&ada);
        let file = new_owned_id(&bob);
        scenario.add(
            &ada,
            1,
            Op::ChannelCreate {
                id: channel.clone(),
                name: "general".into(),
            },
        );
        scenario.add(&bob, 2, shared_file(&file, &channel, "IMG_1.png"));
        scenario.add(
            &bob,
            3,
            Op::FileRename {
                id: file.clone(),
                name: "draft.png".into(),
            },
        );
        scenario.add(&cat, 4, message(&new_owned_id(&cat), &channel, "nice"));
        scenario.add(
            &bob,
            5,
            Op::FileRename {
                id: file.clone(),
                name: "final.png".into(),
            },
        );
        assert_eq!(scenario.assert_files_converge(), [(file, "final.png".to_owned())]);
    }

    #[test]
    fn a_deleted_file_stays_deleted_whatever_its_renames_say() {
        let [ada, bob] = people();
        let mut scenario = Scenario::new();
        let channel = new_owned_id(&ada);
        let file = new_owned_id(&bob);
        scenario.add(
            &ada,
            1,
            Op::ChannelCreate {
                id: channel.clone(),
                name: "general".into(),
            },
        );
        scenario.add(&bob, 2, shared_file(&file, &channel, "notes.txt"));
        scenario.add(&ada, 3, Op::Delete { id: file.clone() });
        scenario.add(
            &bob,
            4,
            Op::FileRename {
                id: file,
                name: "renamed.txt".into(),
            },
        );
        assert!(scenario.assert_files_converge().is_empty());
    }
}
