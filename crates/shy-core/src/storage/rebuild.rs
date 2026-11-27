//! Taking ops out of the log that should never have applied, and rebuilding
//! the shared views from what remains. Device-local tables (read markers,
//! drafts, Later, sidebar) are kept.

use std::collections::HashMap;

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::{ApplyContext, Settling, Store};
use crate::{
    ops::{MAX_ID_CHARS, MAX_STORED_NUMBER, Op, RuleViolation, VerifiedOp},
    pages::{self, PageMerge},
    private::ids::is_private_id,
    views::Change,
};

/// What freezing logs left for the views, until it is done.
const VIEW_WORK_META: &str = "view_work_after_freeze";

#[derive(Debug, Default, Serialize, Deserialize)]
struct ViewWork {
    rebuild: bool,
    settle: Vec<PublicKey>,
}

/// Tables that hold only what ops produce.
pub(super) const SHARED_VIEW_TABLES: &[&str] = &[
    "profiles",
    "profile_details",
    "channels",
    "messages",
    "reactions",
    "pages",
    "search",
    "search_rows",
    "message_edits",
    "channel_names",
    "deleted",
    "member_status",
    "sealing_keys",
    "files",
    "profile_pictures",
    "file_names",
    "workspace_icon",
    "workspace_look",
    "device_changes",
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
    /// Returns whether the views changed.
    pub fn freeze_author(
        &self,
        member: &PublicKey,
        last_seq: u64,
        context: ApplyContext<'_>,
    ) -> Result<bool> {
        self.atomically(|store| {
            if store.freeze_log(member, last_seq, None)? {
                store.rebuild_views(context)?;
                return Ok(true);
            }
            store.settle_after_removal(member, context)
        })
    }

    /// Records where a removed member's log ends and takes their later ops
    /// out of the log, leaving the views for [`Self::finish_view_work`].
    /// Returns whether any op was taken out.
    pub(crate) fn freeze_log(
        &self,
        member: &PublicKey,
        last_seq: u64,
        frozen_epoch: Option<u64>,
    ) -> Result<bool> {
        // No op has a seq past what the store holds (receiving refuses
        // them), so freezing past it means the same as freezing there.
        let last_seq = last_seq.min(MAX_STORED_NUMBER);
        self.record_frozen_author(member, last_seq, frozen_epoch)?;
        let dropped = self.db.execute(
            "DELETE FROM ops WHERE author = ? AND seq > ?",
            params![member.as_bytes(), last_seq],
        )?;
        Ok(dropped > 0)
    }

    /// Notes what the views still need after freezing these logs: a rebuild
    /// if ops were taken out, else settling each removal.
    pub(crate) fn add_view_work(&self, frozen: &[(PublicKey, bool)]) -> Result<()> {
        let mut work = self.view_work()?;
        for (member, was_dropped) in frozen {
            match was_dropped {
                true => work.rebuild = true,
                false => work.settle.push(*member),
            }
        }
        self.set_meta(VIEW_WORK_META, &postcard::to_stdvec(&work)?)
    }

    /// Does what freezing left for the views. It can be slow or fail (a full
    /// disk), so it runs apart from following commits and is retried at
    /// every pass. Returns whether the views changed.
    pub(crate) fn finish_view_work(&self, context: ApplyContext<'_>) -> Result<bool> {
        let work = self.view_work()?;
        if !work.rebuild && work.settle.is_empty() {
            return Ok(false);
        }
        self.atomically(|store| {
            let mut changed = false;
            if work.rebuild {
                store.rebuild_views(context)?;
                changed = true;
            } else {
                for member in &work.settle {
                    changed |= store.settle_after_removal(member, context)?;
                }
            }
            store.delete_meta(VIEW_WORK_META)?;
            Ok(changed)
        })
    }

    /// Whether ops a removed member should never have had are out of the
    /// log but still in the views.
    pub(crate) fn has_view_rebuild_pending(&self) -> Result<bool> {
        Ok(self.view_work()?.rebuild)
    }

    fn view_work(&self) -> Result<ViewWork> {
        Ok(match self.meta(VIEW_WORK_META)? {
            Some(bytes) => postcard::from_bytes(&bytes)?,
            None => ViewWork::default(),
        })
    }

    /// Retries every waiting private op once `member`'s removal is settled
    /// (it was frozen and their log up to the cut has arrived): some may wait
    /// for exactly that, like a claim on a channel they owned. Returns
    /// whether the views changed.
    pub(crate) fn settle_after_removal(&self, member: &PublicKey, context: ApplyContext<'_>) -> Result<bool> {
        let Some(cut) = self.frozen_last_seq(member)? else {
            return Ok(false);
        };
        if self.last_seq(member)? < cut {
            return Ok(false);
        }
        let applied = self.retry_sealed(&self.waiting_channels()?, context)?;
        if applied.needs_rebuild {
            self.rebuild_views(context)?;
            return Ok(true);
        }
        Ok(!applied.changes.is_empty())
    }

    /// Replays every stored op into empty views and private state, in the
    /// order they were first applied. Ops merge the same way in any order, so
    /// this gives what a device that never saw the dropped ops has. Sealed ops
    /// then settle in phases: the owner logs, then leaves, then content, each
    /// in its authors' order. Runs inside a transaction.
    pub(crate) fn rebuild_views(&self, context: ApplyContext<'_>) -> Result<()> {
        self.replace_pending_pages();
        for table in SHARED_VIEW_TABLES {
            self.db.execute(&format!("DELETE FROM {table}"), [])?;
        }
        // Replaying queues again the removals of deletes that still count;
        // one whose delete no longer counts must not run.
        self.db.execute("DELETE FROM blob_removals", [])?;
        self.clear_private_state()?;
        let settled = [
            Settling::Nothing,
            Settling::OwnerOps,
            Settling::Leaves,
            Settling::Everything,
        ]
        .into_iter()
        .try_for_each(|phase| {
            self.settling.set(phase);
            match phase {
                Settling::Nothing => self.replay_log(context),
                _ => self.retry_sealed(&self.waiting_channels()?, context).map(drop),
            }
        });
        self.settling.set(Settling::Everything);
        settled
    }

    fn replay_log(&self, context: ApplyContext<'_>) -> Result<()> {
        let stored: Vec<Vec<u8>> = self
            .db
            .prepare("SELECT envelope FROM ops ORDER BY rowid")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut pages: HashMap<String, PageReplay> = HashMap::new();
        for bytes in stored {
            let Some(verified) = self.open_stored(&bytes)? else {
                continue;
            };
            match &verified.body.op {
                Op::DocUpdate { page, nonce, update } => {
                    let client = pages::client_id(&verified.author, page, nonce);
                    let timestamp_ms = super::apply::shown_time(&verified)?;
                    self.gather_page_update(&mut pages, page, update, client, timestamp_ms)?;
                }
                _ => self.replay(&verified, context)?,
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

    /// Everything a rebuild may have changed, for anything showing it.
    pub(crate) fn all_view_changes(&self) -> Result<Vec<Change>> {
        let mut changes = vec![
            Change::Channels,
            Change::Pages,
            Change::Members,
            Change::Workspaces,
            Change::WorkspaceLook,
        ];
        for channel in self.channels()? {
            changes.push(Change::Messages { channel: channel.id });
        }
        // Open editors can't drop what they already merged; they start over.
        for page in self.pages()? {
            changes.push(Change::PageReset { page: page.id });
        }
        Ok(changes)
    }

    /// Applies one op; one that breaks a rule leaves no trace, as when it
    /// first arrived.
    fn replay(&self, verified: &VerifiedOp, context: ApplyContext<'_>) -> Result<()> {
        match self.with_savepoint(|store| store.apply_permitted(verified, context)) {
            Ok(_) => Ok(()),
            Err(error) if error.downcast_ref::<RuleViolation>().is_some() => {
                warn!("skipping a stored op that breaks a rule: {error:#}");
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Like applying a page update, but merged in memory with the page's other
    /// updates. A page deleted by now stays deleted, as deletes win.
    fn gather_page_update(
        &self,
        pages: &mut HashMap<String, PageReplay>,
        page: &str,
        update: &[u8],
        client: u64,
        timestamp_ms: i64,
    ) -> Result<()> {
        // The rules apply_permitted holds ordinary ops to, for page updates
        // that skip it here.
        if is_private_id(page) || page.len() > MAX_ID_CHARS || self.is_deleted(page)? {
            return Ok(());
        }
        let mut replay = pages.remove(page).unwrap_or_default();
        let added = replay.merge.add(update, client);
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
