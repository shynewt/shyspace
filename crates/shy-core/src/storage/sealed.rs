//! Sealed ops still waiting to apply, and which ones did.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::{Store, private::read_key};
use crate::private::crypto::KeyHint;

/// A sealed op we couldn't apply yet. `channel` is set once one of our keys
/// matched its hint.
pub struct WaitingSealed {
    pub author: PublicKey,
    pub seq: u64,
    pub hint: KeyHint,
    pub channel: Option<String>,
}

impl Store {
    pub(crate) fn put_waiting_sealed(&self, waiting: &WaitingSealed) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO sealed_pending (author, seq, hint, channel) VALUES (?, ?, ?, ?)",
            params![
                waiting.author.as_bytes(),
                waiting.seq,
                waiting.hint.as_slice(),
                waiting.channel
            ],
        )?;
        Ok(())
    }

    pub(crate) fn match_waiting_sealed(&self, author: &PublicKey, seq: u64, channel: &str) -> Result<()> {
        self.db.execute(
            "UPDATE sealed_pending SET channel = ? WHERE author = ? AND seq = ?",
            params![channel, author.as_bytes(), seq],
        )?;
        Ok(())
    }

    /// Waiting sealed ops no key matched yet, oldest first.
    pub(crate) fn unmatched_sealed(&self) -> Result<Vec<WaitingSealed>> {
        self.waiting(
            "SELECT author, seq, hint, channel FROM sealed_pending WHERE channel IS NULL ORDER BY rowid",
            [],
        )
    }

    /// Waiting sealed ops of `channel`, in each author's order.
    pub(crate) fn waiting_in(&self, channel: &str) -> Result<Vec<WaitingSealed>> {
        self.waiting(
            "SELECT author, seq, hint, channel FROM sealed_pending WHERE channel = ? ORDER BY author, seq",
            [channel],
        )
    }

    fn waiting(&self, query: &str, parameters: impl rusqlite::Params) -> Result<Vec<WaitingSealed>> {
        let mut statement = self.db.prepare(query)?;
        let waiting = statement
            .query_map(parameters, |row| {
                let hint: Vec<u8> = row.get(2)?;
                Ok(WaitingSealed {
                    author: read_key(row.get(0)?)?,
                    seq: row.get(1)?,
                    hint: hint.try_into().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    channel: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(waiting)
    }

    /// Channels with sealed ops waiting.
    pub(crate) fn waiting_channels(&self) -> Result<std::collections::BTreeSet<String>> {
        let mut statement = self
            .db
            .prepare("SELECT DISTINCT channel FROM sealed_pending WHERE channel IS NOT NULL")?;
        let channels = statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(channels)
    }

    pub(crate) fn delete_waiting_sealed(&self, author: &PublicKey, seq: u64) -> Result<()> {
        self.db.execute(
            "DELETE FROM sealed_pending WHERE author = ? AND seq = ?",
            params![author.as_bytes(), seq],
        )?;
        Ok(())
    }

    pub(crate) fn record_private_applied(&self, channel: &str, author: &PublicKey, seq: u64) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO private_applied (channel, author, seq) VALUES (?, ?, ?)",
            params![channel, author.as_bytes(), seq],
        )?;
        Ok(())
    }

    pub fn is_private_applied(&self, author: &PublicKey, seq: u64) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM private_applied WHERE author = ? AND seq = ?",
                params![author.as_bytes(), seq],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Whether ops of `member`'s after `seq` in `channel` were applied here.
    pub(crate) fn has_applied_after(&self, channel: &str, member: &PublicKey, seq: u64) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM private_applied WHERE channel = ? AND author = ? AND seq > ?",
                params![channel, member.as_bytes(), seq],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}
