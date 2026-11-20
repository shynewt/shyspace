//! Taking ops out of the log that should never have applied, and rebuilding
//! the shared views from what remains. Device-local tables (read markers,
//! drafts, Later, sidebar) are kept.

use std::collections::HashMap;

use anyhow::{Context, Result};
use iroh::PublicKey;
use rusqlite::params;
use tracing::warn;

use super::Store;
use crate::{
    crypto,
    ops::{Envelope, Op, RuleViolation, VerifiedOp},
    pages::PageMerge,
};

/// Tables that hold only what ops produce.
const SHARED_VIEW_TABLES: &[&str] = &[
    "profiles",
    "channels",
    "messages",
    "reactions",
    "pages",
    "search",
    "message_edits",
    "channel_names",
    "deleted",
    "member_status",
];

/// A page's updates gathered during a replay, written once at the end.
#[derive(Default)]
struct PageReplay {
    merge: PageMerge,
    updated_ms: i64,
}

impl Store {
    /// Ends a removed member's log at `last_seq`. Ops of theirs after it that
    /// already applied here (sent live just before the removal arrived) are
    /// undone, so this device ends up like those that never saw them.
    /// Returns whether the views were rebuilt.
    pub fn freeze_author(&self, member: &PublicKey, last_seq: u64, admin: &PublicKey) -> Result<bool> {
        self.atomically(|store| {
            store.record_frozen_author(member, last_seq)?;
            let dropped = store.db.execute(
                "DELETE FROM ops WHERE author = ? AND seq > ?",
                params![member.as_bytes(), last_seq],
            )?;
            if dropped == 0 {
                return Ok(false);
            }
            store.rebuild_views(admin)?;
            Ok(true)
        })
    }

    /// Replays every stored op, in the order they were first applied, into
    /// empty views. Ops merge the same way in any order, so this gives what a
    /// device that never saw the dropped ops has. Runs inside a transaction.
    fn rebuild_views(&self, admin: &PublicKey) -> Result<()> {
        for table in SHARED_VIEW_TABLES {
            self.db.execute(&format!("DELETE FROM {table}"), [])?;
        }
        let stored: Vec<Vec<u8>> = self
            .db
            .prepare("SELECT envelope FROM ops ORDER BY rowid")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut pages: HashMap<String, PageReplay> = HashMap::new();
        for bytes in stored {
            let verified = self.open_stored(&bytes)?;
            match &verified.body.op {
                Op::DocUpdate { page, update } => {
                    self.gather_page_update(&mut pages, page, update, verified.body.timestamp_ms)?;
                }
                _ => self.replay(&verified, admin)?,
            }
        }
        for (page, replay) in pages {
            if self.is_deleted(&page)? {
                continue;
            }
            self.db.execute(
                "UPDATE pages SET doc = ?, updated_ms = MAX(updated_ms, ?) WHERE id = ?",
                params![replay.merge.state(), replay.updated_ms, page],
            )?;
            self.reindex_page(&page)?;
        }
        Ok(())
    }

    fn open_stored(&self, bytes: &[u8]) -> Result<VerifiedOp> {
        let envelope: Envelope = postcard::from_bytes(bytes).context("reading a stored op")?;
        let key = self
            .epoch_key(envelope.epoch)?
            .with_context(|| format!("no key for epoch {} of a stored op", envelope.epoch))?;
        crypto::open(&envelope, &key).context("opening a stored op")
    }

    /// Applies one op; one that breaks a rule leaves no trace, as when it
    /// first arrived.
    fn replay(&self, verified: &VerifiedOp, admin: &PublicKey) -> Result<()> {
        self.db.execute_batch("SAVEPOINT replay_op")?;
        match self.apply_permitted(verified, admin) {
            Ok(_) => self.db.execute_batch("RELEASE replay_op")?,
            Err(error) if error.downcast_ref::<RuleViolation>().is_some() => {
                warn!("skipping a stored op that breaks a rule: {error:#}");
                self.db
                    .execute_batch("ROLLBACK TO replay_op; RELEASE replay_op")?;
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }

    /// Like applying a page update, but merged in memory with the page's other
    /// updates. A page deleted by now stays deleted, as deletes win.
    fn gather_page_update(
        &self,
        pages: &mut HashMap<String, PageReplay>,
        page: &str,
        update: &[u8],
        timestamp_ms: i64,
    ) -> Result<()> {
        if self.is_deleted(page)? {
            return Ok(());
        }
        let mut replay = pages.remove(page).unwrap_or_default();
        let added = replay.merge.add(update);
        if added.is_ok() {
            replay.updated_ms = replay.updated_ms.max(timestamp_ms);
        }
        // Only pages with an update that applied get a row, as live.
        if !replay.merge.is_empty() {
            pages.insert(page.to_owned(), replay);
        }
        if let Err(error) = added {
            warn!("skipping a stored page update that is damaged: {error:#}");
            return Ok(());
        }
        self.db
            .execute("INSERT OR IGNORE INTO pages (id) VALUES (?)", [page])?;
        Ok(())
    }
}
