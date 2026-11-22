//! Personal state, kept on this device only and never sent to others: read
//! positions (the last message read in each channel) and alert levels.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::{
    mentions::{GROUP_MENTION_PREFIX, MEMBER_LINK_PREFIX, mentions},
    views::{MessageCursor, NotifyLevel, member_id},
};

/// Messages in channel `?1` by others than `?2`, after the read position
/// (`?3`, `?4`). Comparing as a pair lets SQLite start reading the channel's
/// index there instead of at the beginning.
const UNREAD_BY_OTHERS: &str = "m.channel = ?1 AND m.author != ?2 AND (m.lamport, m.author) > (?3, ?4)";
/// Whether message `m` shows in its channel: it isn't a reply, or its
/// parent hasn't arrived.
const IS_SHOWN_IN_CHANNEL: &str =
    "(m.reply_to IS NULL OR NOT EXISTS (SELECT 1 FROM messages parent WHERE parent.id = m.reply_to))";

/// An unread message that mentions you.
pub struct UnreadMention {
    pub id: String,
    pub lamport: u64,
    /// The author's member id (hex).
    pub author: String,
}

impl Store {
    /// Moves the read position in `channel` forward to `message`. A stale
    /// call (say, from a view that hadn't refreshed) never moves it back.
    pub fn mark_read(&self, channel: &str, message: &str) -> Result<()> {
        let position: Option<(u64, Vec<u8>)> = self
            .db
            .query_row(
                "SELECT lamport, author FROM messages WHERE id = ? AND channel = ?",
                [message, channel],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((lamport, author)) = position else {
            bail!("no such message in this channel");
        };
        self.db.execute(
            "INSERT INTO read_markers (channel, lamport, author) VALUES (?1, ?2, ?3)
             ON CONFLICT (channel) DO UPDATE SET lamport = ?2, author = ?3
             WHERE ?2 > lamport OR (?2 = lamport AND ?3 > author)",
            params![channel, lamport, author],
        )?;
        Ok(())
    }

    pub fn read_marker(&self, channel: &str) -> Result<Option<MessageCursor>> {
        let marker: Option<(u64, Vec<u8>)> = self
            .db
            .query_row(
                "SELECT lamport, author FROM read_markers WHERE channel = ?",
                [channel],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(marker.map(|(lamport, author)| MessageCursor {
            lamport,
            author: hex::encode(author),
        }))
    }

    /// How many messages by others shown in the channel (not thread
    /// replies) come after the read position.
    pub fn unread_count(&self, channel: &str, me: &PublicKey) -> Result<u64> {
        let (lamport, author) = self.read_position(channel)?;
        Ok(self.db.query_row(
            &format!("SELECT COUNT(*) FROM messages m WHERE {UNREAD_BY_OTHERS} AND {IS_SHOWN_IN_CHANNEL}"),
            params![channel, me.as_bytes(), lamport, author],
            |row| row.get(0),
        )?)
    }

    /// Unread messages by others that mention you, oldest first. A thread
    /// reply counts until its thread is read past it.
    pub fn unread_mentions(&self, channel: &str, me: &PublicKey) -> Result<Vec<UnreadMention>> {
        let my_id = member_id(me);
        let (lamport, author) = self.read_position(channel)?;
        // Only messages holding a mention link can mention anyone.
        let mut statement = self.db.prepare(&format!(
            "SELECT m.id, COALESCE(e.text, m.text), {IS_SHOWN_IN_CHANNEL}, m.lamport, m.author
             FROM messages m
             LEFT JOIN message_edits e ON e.message = m.id
             WHERE {UNREAD_BY_OTHERS}
               AND (instr(COALESCE(e.text, m.text), ?5) > 0 OR instr(COALESCE(e.text, m.text), ?6) > 0)
             ORDER BY m.lamport, m.author, m.id"
        ))?;
        let rows = statement.query_map(
            params![
                channel,
                me.as_bytes(),
                lamport,
                author,
                format!("{MEMBER_LINK_PREFIX}{my_id}"),
                GROUP_MENTION_PREFIX
            ],
            |row| {
                let text: String = row.get(1)?;
                let is_in_channel: bool = row.get(2)?;
                let mention = UnreadMention {
                    id: row.get(0)?,
                    lamport: row.get(3)?,
                    author: hex::encode(row.get::<_, Vec<u8>>(4)?),
                };
                Ok((mention, text, is_in_channel))
            },
        )?;
        let mut found = Vec::new();
        let mut thread_roots: Option<HashMap<String, String>> = None;
        for row in rows {
            let (mention, text, is_in_channel) = row?;
            if !mentions(&text, &my_id) {
                continue;
            }
            if !is_in_channel {
                let roots = match &mut thread_roots {
                    Some(roots) => roots,
                    None => thread_roots.insert(self.thread_roots_of_replies(channel)?),
                };
                if self.is_read_in_thread(&mention, roots)? {
                    continue;
                }
            }
            found.push(mention);
        }
        Ok(found)
    }

    /// The read position in `channel` as stored, or one before every
    /// message if there is none.
    fn read_position(&self, channel: &str) -> Result<(i64, Vec<u8>)> {
        let position = self
            .db
            .query_row(
                "SELECT lamport, author FROM read_markers WHERE channel = ?",
                [channel],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(position.unwrap_or((-1, Vec::new())))
    }

    /// Each reply in `channel` mapped to its thread's first message, found
    /// in one pass rather than by walking up from every reply.
    fn thread_roots_of_replies(&self, channel: &str) -> Result<HashMap<String, String>> {
        Ok(self
            .thread_replies(channel)?
            .into_iter()
            .map(|reply| (reply.id, reply.root))
            .collect())
    }

    fn is_read_in_thread(&self, reply: &UnreadMention, roots: &HashMap<String, String>) -> Result<bool> {
        // Not in the channel's threads: either its parent sits in another
        // channel (thread_root then gives the reply itself), or it is in no
        // thread at all and shows nowhere, so there's nothing to read.
        let root = match roots.get(&reply.id) {
            Some(root) => root.clone(),
            None => match self.thread_root(&reply.id)? {
                Some(root) => root,
                None => return Ok(true),
            },
        };
        Ok(self.read_marker(&root)?.is_some_and(|read| {
            (reply.lamport, reply.author.as_str()) <= (read.lamport, read.author.as_str())
        }))
    }

    /// Moves your read position in the thread starting at `root` forward to
    /// `message`, a message in that thread.
    pub fn mark_thread_read(&self, root: &str, message: &str) -> Result<()> {
        if self.thread_root(message)?.as_deref() != Some(root) {
            bail!("that message isn't in this thread");
        }
        let (lamport, author): (u64, Vec<u8>) = self.db.query_row(
            "SELECT lamport, author FROM messages WHERE id = ?",
            [message],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        // Thread positions share the table, keyed by the thread's first message.
        self.db.execute(
            "INSERT INTO read_markers (channel, lamport, author) VALUES (?1, ?2, ?3)
             ON CONFLICT (channel) DO UPDATE SET lamport = ?2, author = ?3
             WHERE ?2 > lamport OR (?2 = lamport AND ?3 > author)",
            params![root, lamport, author],
        )?;
        Ok(())
    }

    pub fn notify_level(&self, channel: &str) -> Result<NotifyLevel> {
        let level: Option<String> = self
            .db
            .query_row(
                "SELECT level FROM channel_notify WHERE channel = ?",
                [channel],
                |row| row.get(0),
            )
            .optional()?;
        match level {
            None => Ok(NotifyLevel::default()),
            Some(text) => NotifyLevel::parse(&text).with_context(|| format!("unknown alert level {text:?}")),
        }
    }

    pub fn set_notify_level(&self, channel: &str, level: NotifyLevel) -> Result<()> {
        self.db.execute(
            "INSERT INTO channel_notify (channel, level) VALUES (?1, ?2)
             ON CONFLICT (channel) DO UPDATE SET level = ?2",
            params![channel, level.as_str()],
        )?;
        Ok(())
    }

    /// Keeps `text` as the draft for `target` (a channel, or a thread's first
    /// message); blank text removes it.
    pub fn set_draft(&self, target: &str, text: &str, now_ms: i64) -> Result<()> {
        if text.trim().is_empty() {
            self.db.execute("DELETE FROM drafts WHERE target = ?", [target])?;
        } else {
            self.db.execute(
                "INSERT INTO drafts (target, text, updated_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT (target) DO UPDATE SET text = ?2, updated_ms = ?3",
                params![target, text, now_ms],
            )?;
        }
        Ok(())
    }

    /// Every draft (target, text, when it last changed), newest first.
    pub fn drafts(&self) -> Result<Vec<(String, String, i64)>> {
        let mut statement = self
            .db
            .prepare("SELECT target, text, updated_ms FROM drafts ORDER BY updated_ms DESC")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use crate::{mentions::member_link, storage::test_channel::TestChannel};

    #[test]
    fn unread_counts_leave_out_thread_replies_and_what_is_read() {
        let mut channel = TestChannel::new();
        let me = SecretKey::generate().public();
        let read = channel.post("already read", None);
        channel.store.mark_read(&channel.id, &read).unwrap();
        let root = channel.post("new", None);
        channel.post("a reply, shown in its thread", Some(&root));
        channel.post("its parent never arrived, so it shows here", Some("missing"));

        assert_eq!(channel.store.unread_count(&channel.id, &me).unwrap(), 2);
        assert_eq!(
            channel.store.unread_count(&channel.id, &channel.author).unwrap(),
            0,
            "your own messages"
        );
    }

    #[test]
    fn unread_mentions_count_replies_until_their_thread_is_read() {
        let mut channel = TestChannel::new();
        let me = SecretKey::generate().public();
        let mention = member_link("Me", &hex::encode(me.as_bytes()));
        let someone_else = member_link("Other", &hex::encode(channel.author.as_bytes()));
        let root = channel.post(&format!("hi {mention}"), None);
        let reply = channel.post(&format!("again {mention}"), Some(&root));
        channel.post(&format!("hi {someone_else}"), None);
        let everyone = channel.post("[@channel](shy://mention/channel) news", None);

        let found = |channel: &TestChannel| -> Vec<String> {
            channel
                .store
                .unread_mentions(&channel.id, &me)
                .unwrap()
                .into_iter()
                .map(|mention| mention.id)
                .collect()
        };
        assert_eq!(found(&channel), [root.clone(), reply.clone(), everyone.clone()]);
        channel.store.mark_thread_read(&root, &reply).unwrap();
        assert_eq!(found(&channel), [root, everyone]);
    }

    #[test]
    fn counting_unread_costs_what_is_unread_not_the_whole_channel() {
        let mut channel = TestChannel::new();
        let me = SecretKey::generate().public();
        let read_up_to = |channel: &mut TestChannel, count: usize| {
            let mut last = String::new();
            for _ in 0..count {
                last = channel.post("old news", None);
            }
            channel.store.mark_read(&channel.id, &last).unwrap();
            for _ in 0..10 {
                channel.post("new", None);
            }
        };
        let cost = |channel: &TestChannel| {
            channel.store.steps_of(|| {
                channel.store.unread_count(&channel.id, &me).unwrap();
                channel.store.unread_mentions(&channel.id, &me).unwrap();
            })
        };
        read_up_to(&mut channel, 20);
        let with_little_read = cost(&channel);
        read_up_to(&mut channel, 2_000);
        let with_much_read = cost(&channel);

        assert_eq!(channel.store.unread_count(&channel.id, &me).unwrap(), 10);
        assert!(
            with_much_read < with_little_read * 2,
            "{with_little_read} steps with 30 messages, {with_much_read} with 2,040"
        );
    }

    #[test]
    fn counting_unread_in_a_long_thread_grows_with_its_length_not_its_square() {
        let mut channel = TestChannel::new();
        let me = SecretKey::generate().public();
        let mention = member_link("Me", &hex::encode(me.as_bytes()));
        let mut last = channel.post("first", None);
        let mut extend_thread = |channel: &mut TestChannel, count: usize| {
            for _ in 0..count {
                last = channel.post(&format!("and {mention}"), Some(&last));
            }
        };
        let cost = |channel: &TestChannel| {
            channel.store.steps_of(|| {
                channel.store.unread_count(&channel.id, &me).unwrap();
                channel.store.unread_mentions(&channel.id, &me).unwrap();
            })
        };
        extend_thread(&mut channel, 100);
        let with_short = cost(&channel);
        extend_thread(&mut channel, 300);
        let with_four_times = cost(&channel);

        assert!(
            with_four_times < with_short * 6,
            "{with_short} steps for 100 replies, {with_four_times} for 400"
        );
    }
}
