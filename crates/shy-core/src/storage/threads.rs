//! Threads. A message that replies to another lives in the thread of the
//! message it (eventually) answers, in the same channel. Roots are worked
//! out when reading, so a reply that arrives before its parent still lands
//! in the right place; one whose parent is missing shows in the channel.

use anyhow::{Result, bail};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::{
    Store,
    queries::{MESSAGE_COLUMNS, MESSAGE_JOINS, message_from_row},
};
use crate::views::Message;

/// The longest reply chain followed when looking for a thread's root.
const MAX_THREAD_DEPTH: usize = 1000;

/// A reply, with the first message of its thread.
pub struct Reply {
    pub id: String,
    pub root: String,
    /// The author's member id (hex).
    pub author: String,
    pub lamport: u64,
    pub created_ms: i64,
    pub text: String,
}

impl Store {
    /// Every reply in `channel`, oldest first, with its thread's root.
    pub fn thread_replies(&self, channel: &str) -> Result<Vec<Reply>> {
        // UNION, not UNION ALL: a reply loop made on purpose still ends.
        let mut statement = self.db.prepare(
            "WITH RECURSIVE chain(id, root) AS (
                SELECT m.id, m.id FROM messages m
                WHERE m.channel = ?1
                  AND (m.reply_to IS NULL OR NOT EXISTS (SELECT 1 FROM messages parent WHERE parent.id = m.reply_to))
                UNION
                SELECT m.id, chain.root FROM messages m JOIN chain ON m.reply_to = chain.id
                WHERE m.channel = ?1
             )
             SELECT chain.id, chain.root, m.author, m.lamport, m.created_ms, COALESCE(e.text, m.text)
             FROM chain JOIN messages m ON m.id = chain.id
             LEFT JOIN message_edits e ON e.message = m.id
             WHERE chain.id != chain.root
             ORDER BY m.lamport, m.author",
        )?;
        let rows = statement.query_map([channel], |row| {
            Ok(Reply {
                id: row.get(0)?,
                root: row.get(1)?,
                author: hex::encode(row.get::<_, Vec<u8>>(2)?),
                lamport: row.get(3)?,
                created_ms: row.get(4)?,
                text: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The thread starting at `root`: its first message, then every reply.
    pub fn thread(&self, root: &str, me: &PublicKey) -> Result<Vec<Message>> {
        let mut statement = self.db.prepare(&format!(
            "WITH RECURSIVE chain(id) AS (
                SELECT ?1
                UNION
                SELECT m.id FROM messages m JOIN chain ON m.reply_to = chain.id
                WHERE m.channel = (SELECT channel FROM messages WHERE id = ?1)
             )
             SELECT {MESSAGE_COLUMNS} {MESSAGE_JOINS}
             WHERE m.id IN (SELECT id FROM chain)
             ORDER BY m.id != ?1, m.lamport, m.author"
        ))?;
        let rows = statement.query_map([root], message_from_row)?;
        self.with_reactions(rows.collect::<Result<_, _>>()?, me)
    }

    /// The first message of the thread `message` is in (itself if it isn't
    /// a reply), or `None` if `message` doesn't exist.
    pub fn thread_root(&self, message: &str) -> Result<Option<String>> {
        let mut current = message.to_owned();
        for _ in 0..MAX_THREAD_DEPTH {
            let row: Option<(String, Option<String>)> = self
                .db
                .query_row(
                    "SELECT channel, reply_to FROM messages WHERE id = ?",
                    [&current],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((channel, reply_to)) = row else {
                return Ok(None);
            };
            let parent: Option<String> = match reply_to {
                Some(parent) => self
                    .db
                    .query_row(
                        "SELECT id FROM messages WHERE id = ? AND channel = ?",
                        [&parent, &channel],
                        |row| row.get(0),
                    )
                    .optional()?,
                None => None,
            };
            match parent {
                Some(parent) => current = parent,
                None => return Ok(Some(current)),
            }
        }
        bail!("reply chain too long or looping at message {message}")
    }

    /// Your explicit choice to follow a thread or not, if you made one.
    pub fn following_choice(&self, root: &str) -> Result<Option<bool>> {
        Ok(self
            .db
            .query_row(
                "SELECT is_following FROM thread_follows WHERE root = ?",
                [root],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_following(&self, root: &str, is_following: bool) -> Result<()> {
        self.db.execute(
            "INSERT INTO thread_follows (root, is_following) VALUES (?1, ?2)
             ON CONFLICT (root) DO UPDATE SET is_following = ?2",
            params![root, is_following],
        )?;
        Ok(())
    }
}
