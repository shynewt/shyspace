//! Ops we have applied, ops waiting to be applied, and how far we have read
//! each author's log in the bucket.

use anyhow::{Result, bail};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::ops::Envelope;

/// How far ahead of our own clock an op's lamport may be. Without a bound, one
/// op with lamport u64::MAX would pin titles forever and overflow every clock.
const MAX_CLOCK_JUMP: u64 = 1 << 32;
const MAX_PENDING_PER_AUTHOR: u32 = 2_000;
const MAX_PENDING_TOTAL: u32 = 20_000;
const LAMPORT_KEY: &str = "lamport";

impl Store {
    fn lamport(&self) -> Result<u64> {
        let Some(bytes) = self.meta(LAMPORT_KEY)? else {
            return Ok(0);
        };
        let bytes: [u8; 8] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("stored lamport clock is corrupt"))?;
        Ok(u64::from_be_bytes(bytes))
    }

    /// One more than anything we've seen or written.
    pub fn next_lamport(&self) -> Result<u64> {
        let Some(next) = self.lamport()?.checked_add(1) else {
            bail!("lamport clock exhausted");
        };
        self.set_meta(LAMPORT_KEY, &next.to_be_bytes())?;
        Ok(next)
    }

    pub(super) fn observe_lamport(&self, lamport: u64) -> Result<()> {
        if lamport > self.lamport()? {
            self.set_meta(LAMPORT_KEY, &lamport.to_be_bytes())?;
        }
        Ok(())
    }

    /// Whether an op's clock is close enough to ours to accept now. Ops that
    /// jump too far ahead wait as pending until our clock catches up, so
    /// replicas don't permanently disagree about them.
    pub fn accepts_clock(&self, lamport: u64) -> Result<bool> {
        Ok(lamport <= self.lamport()?.saturating_add(MAX_CLOCK_JUMP))
    }

    /// The highest sequence number we have applied from `author`, or 0.
    pub fn last_seq(&self, author: &PublicKey) -> Result<u64> {
        let last: Option<u64> = self.db.query_row(
            "SELECT MAX(seq) FROM ops WHERE author = ?",
            [author.as_bytes()],
            |row| row.get(0),
        )?;
        Ok(last.unwrap_or(0))
    }

    pub fn next_seq(&self, author: &PublicKey) -> Result<u64> {
        Ok(self.last_seq(author)? + 1)
    }

    pub fn is_applied(&self, author: &PublicKey, seq: u64) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM ops WHERE author = ? AND seq = ?",
            params![author.as_bytes(), seq],
        )
    }

    pub fn has_ops_from(&self, author: &PublicKey) -> Result<bool> {
        self.exists("SELECT 1 FROM ops WHERE author = ? LIMIT 1", [author.as_bytes()])
    }

    /// Records an applied envelope and clears any parked copies of it.
    pub fn insert_applied(&self, envelope: &Envelope, is_pushed: bool) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO ops (author, seq, epoch, envelope, pushed) VALUES (?, ?, ?, ?, ?)",
            params![
                envelope.author.as_bytes(),
                envelope.seq,
                envelope.epoch,
                postcard::to_stdvec(envelope)?,
                is_pushed
            ],
        )?;
        self.db.execute(
            "DELETE FROM pending_ops WHERE author = ? AND seq = ?",
            params![envelope.author.as_bytes(), envelope.seq],
        )?;
        Ok(())
    }

    /// Parks an envelope we can't apply yet. Each distinct envelope gets its
    /// own row, so a forged copy can never replace the genuine one.
    pub fn insert_pending(&self, envelope: &Envelope) -> Result<()> {
        let bytes = postcard::to_stdvec(envelope)?;
        self.db.execute(
            "INSERT OR IGNORE INTO pending_ops (hash, author, seq, epoch, envelope) VALUES (?, ?, ?, ?, ?)",
            params![
                blake3::hash(&bytes).as_bytes().as_slice(),
                envelope.author.as_bytes(),
                envelope.seq,
                envelope.epoch,
                bytes
            ],
        )?;
        // Bound what forged envelopes can make us keep: oldest go first.
        self.db.execute(
            "DELETE FROM pending_ops WHERE author = ?1 AND rowid NOT IN
                (SELECT rowid FROM pending_ops WHERE author = ?1 ORDER BY rowid DESC LIMIT ?2)",
            params![envelope.author.as_bytes(), MAX_PENDING_PER_AUTHOR],
        )?;
        self.db.execute(
            "DELETE FROM pending_ops WHERE rowid NOT IN (SELECT rowid FROM pending_ops ORDER BY rowid DESC LIMIT ?)",
            [MAX_PENDING_TOTAL],
        )?;
        Ok(())
    }

    pub fn delete_pending(&self, envelope: &Envelope) -> Result<()> {
        let bytes = postcard::to_stdvec(envelope)?;
        self.db.execute(
            "DELETE FROM pending_ops WHERE hash = ?",
            [blake3::hash(&bytes).as_bytes().as_slice()],
        )?;
        Ok(())
    }

    pub fn pending(&self) -> Result<Vec<Envelope>> {
        self.envelopes("SELECT envelope FROM pending_ops ORDER BY epoch, seq", [])
    }

    pub fn unpushed(&self, author: &PublicKey) -> Result<Vec<Envelope>> {
        self.envelopes(
            "SELECT envelope FROM ops WHERE author = ? AND pushed = 0 ORDER BY seq",
            [author.as_bytes()],
        )
    }

    pub fn mark_pushed(&self, author: &PublicKey, up_to_seq: u64) -> Result<()> {
        self.db.execute(
            "UPDATE ops SET pushed = 1 WHERE author = ? AND seq <= ?",
            params![author.as_bytes(), up_to_seq],
        )?;
        Ok(())
    }

    fn envelopes(&self, sql: &str, args: impl rusqlite::Params) -> Result<Vec<Envelope>> {
        let mut statement = self.db.prepare(sql)?;
        let rows = statement.query_map(args, |row| row.get::<_, Vec<u8>>(0))?;
        rows.map(|bytes| Ok(postcard::from_bytes(&bytes?)?)).collect()
    }

    /// The last bucket key we read from `author`'s log.
    pub fn cursor(&self, author: &PublicKey) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT last_key FROM cursors WHERE author = ?",
                [author.as_bytes()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_cursor(&self, author: &PublicKey, last_key: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO cursors (author, last_key) VALUES (?, ?)
             ON CONFLICT (author) DO UPDATE SET last_key = excluded.last_key",
            params![author.as_bytes(), last_key],
        )?;
        Ok(())
    }
}
