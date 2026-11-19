//! Read positions: personal, kept on this device only, never sent to others.
//! A channel's position is the last message read, in channel order.

use anyhow::{Result, bail};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::views::MessageCursor;

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

    /// Messages by others after the read position.
    pub fn unread_count(&self, channel: &str, me: &PublicKey) -> Result<u32> {
        Ok(self.db.query_row(
            "SELECT COUNT(*) FROM messages m LEFT JOIN read_markers r ON r.channel = m.channel
             WHERE m.channel = ?1 AND m.author != ?2
               AND (r.channel IS NULL OR m.lamport > r.lamport OR (m.lamport = r.lamport AND m.author > r.author))",
            params![channel, me.as_bytes()],
            |row| row.get(0),
        )?)
    }
}
