//! Personal state, kept on this device only and never sent to others: read
//! positions (the last message read in each channel) and alert levels.

use anyhow::{Context, Result, bail};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::views::{MessageCursor, NotifyLevel};

pub struct UnreadMessage {
    pub id: String,
    pub text: String,
    /// False for a thread reply, which shows in its thread instead.
    pub is_in_channel: bool,
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

    /// Messages by others after the read position, oldest first: id,
    /// current text, and whether it shows in the channel (not a thread reply).
    pub fn unread_messages(&self, channel: &str, me: &PublicKey) -> Result<Vec<UnreadMessage>> {
        let mut statement = self.db.prepare(
            "SELECT m.id, COALESCE(e.text, m.text),
                    m.reply_to IS NULL OR NOT EXISTS (SELECT 1 FROM messages parent WHERE parent.id = m.reply_to)
             FROM messages m
             LEFT JOIN message_edits e ON e.message = m.id
             LEFT JOIN read_markers r ON r.channel = m.channel
             WHERE m.channel = ?1 AND m.author != ?2
               AND (r.channel IS NULL OR m.lamport > r.lamport OR (m.lamport = r.lamport AND m.author > r.author))
             ORDER BY m.lamport, m.author",
        )?;
        let rows = statement.query_map(params![channel, me.as_bytes()], |row| {
            Ok(UnreadMessage {
                id: row.get(0)?,
                text: row.get(1)?,
                is_in_channel: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
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
