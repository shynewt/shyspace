//! Reading again what may hold ops we lack: gap repair, and the objects
//! skipped this run because they didn't settle.

use anyhow::Result;
use iroh::PublicKey;
use tracing::warn;

use crate::{transport::bucket::ListedObject, views::member_id, workspace::Workspace};

impl Workspace {
    /// Gaps not yet repaired: an author's next op went missing after we
    /// read past it (a parked op pushed out by the cap). What was skipped of
    /// their segments this run is fetched again. A gap counts as repaired
    /// once a read of its author succeeds (`mark_gap_repaired`), so one the
    /// bucket can't fill isn't read again at every pass, and one a network
    /// error interrupted is.
    pub(super) fn gaps_to_repair(&self) -> Result<Vec<(PublicKey, u64)>> {
        let missing = self.state().store.authors_missing_next_op()?;
        let repaired = self
            .inner
            .refetched_gaps
            .lock()
            .expect("gap list lock poisoned by an earlier panic")
            .clone();
        let mut to_repair = Vec::new();
        for (author, next_seq) in missing {
            if repaired.contains(&(author, next_seq)) {
                continue;
            }
            warn!("op {next_seq} from {author} went missing; reading their history again");
            self.forget_skipped_segments_of(&author);
            to_repair.push((author, next_seq));
        }
        Ok(to_repair)
    }

    pub(super) fn mark_gap_repaired(&self, gap: (PublicKey, u64)) {
        self.inner
            .refetched_gaps
            .lock()
            .expect("gap list lock poisoned by an earlier panic")
            .insert(gap);
    }

    /// Forgets which objects didn't settle this run. Called when a new epoch
    /// key arrives: ops that waited for it may settle now, and gap repair
    /// can't be relied on to notice if forged parked ops pushed the real ones
    /// out. Junk is then fetched again once per new key.
    pub(crate) fn forget_skipped_segments(&self) {
        self.skipped_segments().clear();
    }

    /// Gives `author`'s segments that were given up this run (junk, or ones
    /// that kept failing to download) fresh tries.
    pub(super) fn forget_given_up_segments_of(&self, author: &PublicKey) {
        let author_prefix = format!("ops/{}/", member_id(author));
        self.skipped_segments()
            .retain(|listed| !listed.key.starts_with(&author_prefix));
        self.inner
            .fetch_failures
            .lock()
            .expect("fetch failures lock poisoned by an earlier panic")
            .retain(|listed, _| !listed.key.starts_with(&author_prefix));
    }

    /// Forgets which of `author`'s objects didn't settle this run, so the
    /// next read of their log lists them all again.
    pub(crate) fn forget_skipped_segments_of(&self, author: &PublicKey) {
        let author_prefix = format!("ops/{}/", member_id(author));
        self.skipped_segments()
            .retain(|listed| !listed.key.starts_with(&author_prefix));
    }

    pub(super) fn skipped_segments(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashSet<ListedObject>> {
        self.inner
            .skipped_segments
            .lock()
            .expect("skipped segments lock poisoned by an earlier panic")
    }
}
