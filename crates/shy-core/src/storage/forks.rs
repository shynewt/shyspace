//! Op slots an author signed two different versions of (#109). Neither
//! version counts on any device, so everyone ends up alike whichever one
//! they saw. Rebuilds keep these rows; the bucket's proofs give them back.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::{ApplyContext, Store, private::read_key};
use crate::ops::Envelope;

/// A fork whose proof checked out.
pub(crate) struct CheckedFork {
    pub author: PublicKey,
    pub seq: u64,
    pub proof: Vec<u8>,
}

impl Store {
    pub fn is_forked(&self, author: &PublicKey, seq: u64) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM forks WHERE author = ? AND seq = ?",
            params![author.as_bytes(), seq],
        )
    }

    /// The envelope we applied for `author`'s op `seq`, if any.
    pub(crate) fn applied_envelope(&self, author: &PublicKey, seq: u64) -> Result<Option<Envelope>> {
        let bytes: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT envelope FROM ops WHERE author = ? AND seq = ?",
                params![author.as_bytes(), seq],
                |row| row.get(0),
            )
            .optional()?;
        Ok(bytes.map(|bytes| postcard::from_bytes(&bytes)).transpose()?)
    }

    /// Records checked forks and rebuilds the views once without their ops.
    /// Returns whether any was new.
    pub(crate) fn record_forks(&self, forks: &[CheckedFork], context: ApplyContext<'_>) -> Result<bool> {
        self.atomically(|store| {
            let mut is_new = false;
            for fork in forks {
                is_new |= store.db.execute(
                    "INSERT OR IGNORE INTO forks (author, seq, proof) VALUES (?, ?, ?)",
                    params![fork.author.as_bytes(), fork.seq, fork.proof],
                )? > 0;
            }
            if is_new {
                store.rebuild_views(context)?;
            }
            Ok(is_new)
        })
    }

    /// Whether the fork proof at `path` in the bucket is worth fetching:
    /// one we haven't checked, or one waiting for an epoch key we now have.
    pub(crate) fn should_fetch_fork_proof(&self, path: &str) -> Result<bool> {
        let checked: Option<Option<u64>> = self
            .db
            .query_row(
                "SELECT waiting_for_epoch FROM fork_proofs_checked WHERE path = ?",
                [path],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match checked {
            None => true,
            Some(None) => false,
            Some(Some(epoch)) => self.epoch_key(epoch)?.is_some(),
        })
    }

    /// Remembers a fork proof that proved nothing (`waiting_for_epoch` is
    /// `None`) or that needs that epoch's key first.
    pub(crate) fn mark_fork_proof_checked(&self, path: &str, waiting_for_epoch: Option<u64>) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO fork_proofs_checked (path, waiting_for_epoch) VALUES (?, ?)",
            params![path, waiting_for_epoch],
        )?;
        Ok(())
    }

    /// Every fork we know of, with its proof.
    pub(crate) fn fork_proofs(&self) -> Result<Vec<(PublicKey, u64, Vec<u8>)>> {
        let mut statement = self
            .db
            .prepare("SELECT author, seq, proof FROM forks ORDER BY author, seq")?;
        let forks = statement
            .query_map([], |row| Ok((read_key(row.get(0)?)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(forks)
    }
}
