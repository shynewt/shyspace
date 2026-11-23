//! Ops we have applied, ops waiting to be applied, and how far we have read
//! each author's log in the bucket.

use anyhow::{Context, Result, bail};
use iroh::PublicKey;
use rusqlite::params;

use super::Store;
use crate::{
    crypto,
    ops::{Envelope, VerifiedOp},
};

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

    /// An applied op, opened again from the log.
    pub(crate) fn stored_op(&self, author: &PublicKey, seq: u64) -> Result<VerifiedOp> {
        let bytes: Vec<u8> = self.db.query_row(
            "SELECT envelope FROM ops WHERE author = ? AND seq = ?",
            params![author.as_bytes(), seq],
            |row| row.get(0),
        )?;
        self.open_stored(&bytes)?
            .context("that op never counts: it was refused for its size, or forked")
    }

    /// An op from the log, or `None` for one that never counts: refused for
    /// its size (kept without content, or kept whole by an older version),
    /// or one its author also signed another version of.
    pub(crate) fn open_stored(&self, bytes: &[u8]) -> Result<Option<VerifiedOp>> {
        let envelope: Envelope = postcard::from_bytes(bytes).context("reading a stored op")?;
        if envelope.ciphertext.is_empty() || self.is_forked(&envelope.author, envelope.seq)? {
            return Ok(None);
        }
        let key = self
            .epoch_key(envelope.epoch)?
            .with_context(|| format!("no key for epoch {} of a stored op", envelope.epoch))?;
        let verified = crypto::open_kept(&envelope, &key).context("opening a stored op")?;
        Ok(envelope.fits_size_limit(&verified.body.op).then_some(verified))
    }

    /// Records an op refused for its size as handled, keeping only its
    /// header. Sealing always adds an authentication tag, so empty content
    /// can only mean this.
    pub fn insert_refused(&self, envelope: &Envelope) -> Result<()> {
        let header_only = Envelope {
            author: envelope.author,
            seq: envelope.seq,
            epoch: envelope.epoch,
            nonce: envelope.nonce,
            ciphertext: Vec::new(),
        };
        self.insert_applied(&header_only, true)
    }

    /// Records an applied envelope and clears its parked copy. Other parked
    /// versions of it stay until retrying them checks them for a fork.
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
        self.delete_pending(envelope)
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

    #[cfg(test)]
    pub(crate) fn envelopes_after(&self, author: &PublicKey, seq: u64) -> Result<Vec<Envelope>> {
        self.envelopes(
            "SELECT envelope FROM ops WHERE author = ? AND seq > ? ORDER BY seq",
            params![author.as_bytes(), seq],
        )
    }

    /// `author`'s envelopes from `first` to `last`, in order, as stored.
    pub fn envelopes_between(&self, author: &PublicKey, first: u64, last: u64) -> Result<Vec<Envelope>> {
        self.envelopes(
            "SELECT envelope FROM ops WHERE author = ? AND seq BETWEEN ? AND ? ORDER BY seq",
            params![author.as_bytes(), first, last],
        )
    }

    pub fn unpushed(&self, author: &PublicKey) -> Result<Vec<Envelope>> {
        self.envelopes(
            "SELECT envelope FROM ops WHERE author = ? AND pushed = 0 ORDER BY seq",
            [author.as_bytes()],
        )
    }

    /// The highest seq of `author`'s that is in the bucket, 0 if none.
    pub fn last_pushed_seq(&self, author: &PublicKey) -> Result<u64> {
        let last: Option<u64> = self.db.query_row(
            "SELECT MAX(seq) FROM ops WHERE author = ? AND pushed = 1",
            [author.as_bytes()],
            |row| row.get(0),
        )?;
        Ok(last.unwrap_or(0))
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
    /// Authors with parked ops but not the one that must apply next: it was
    /// lost (say, pushed out of the bounded parking table) after we read
    /// past it in the bucket.
    pub fn authors_missing_next_op(&self) -> Result<Vec<(PublicKey, u64)>> {
        let mut statement = self.db.prepare(
            "SELECT gap.author, gap.next FROM (
                SELECT DISTINCT p.author,
                       COALESCE((SELECT MAX(o.seq) FROM ops o WHERE o.author = p.author), 0) + 1 AS next
                FROM pending_ops p) AS gap
             WHERE NOT EXISTS (SELECT 1 FROM pending_ops q WHERE q.author = gap.author AND q.seq = gap.next)",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, u64>(1)?)))?;
        let mut missing = Vec::new();
        for row in rows {
            let (author, next) = row?;
            let author: [u8; 32] = author
                .try_into()
                .map_err(|_| anyhow::anyhow!("corrupt author key in parked ops"))?;
            missing.push((PublicKey::from_bytes(&author)?, next));
        }
        Ok(missing)
    }
}
