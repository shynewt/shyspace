//! Members apply the admin's commits, in epoch order, as they appear.

use anyhow::{Context, Result, ensure};

use super::{commit, mls::Mls};
use crate::{views::Change, workspace::Workspace};

/// Set once this device has followed a commit that binds its lists, or
/// joined through one.
pub(crate) const BOUND_COMMIT_FOLLOWED_META: &str = "bound_commit_followed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitOutcome {
    MovedOn,
    RemovedSelf,
}

impl Workspace {
    pub(crate) async fn follow_admin_commits(&self) -> Result<()> {
        if self.is_admin() {
            return Ok(());
        }
        loop {
            let epoch = {
                let state = self.state();
                if !state.mls.is_active() {
                    return Ok(());
                }
                state.mls.epoch()?
            };
            let Some(bytes) = self.bucket().commit(epoch).await? else {
                return Ok(());
            };
            if self.apply_admin_commit(epoch, &bytes)? == CommitOutcome::RemovedSelf {
                return Ok(());
            }
        }
    }

    /// Applies the admin's commit that moves us on from `epoch`, wherever
    /// its bytes came from: they're checked against the admin's signature.
    ///
    /// MLS can process a commit only once, so everything that could reject
    /// it comes first, and nothing sits between MLS processing it and
    /// merging it. What the commit froze is settled after the merge, by
    /// [`State::record_epoch`](crate::workspace::State::record_epoch).
    pub(crate) fn apply_admin_commit(&self, epoch: u64, bytes: &[u8]) -> Result<CommitOutcome> {
        let admin = self.inner.config.admin;
        // An epoch left half settled (a crash, a full disk) is finished first.
        self.state().record_epoch(&admin)?;
        let verified = commit::verify(bytes, epoch, &admin)
            .with_context(|| format!("ignoring invalid commit for epoch {epoch}"))?;
        self.check_binding(epoch, &verified)
            .with_context(|| format!("ignoring invalid commit for epoch {epoch}"))?;
        let outcome = {
            let mut state = self.state();
            state.save_commit(epoch, bytes)?;
            let outcome = match state.mls.process_commit(&verified.commit, &admin) {
                Ok(outcome) => outcome,
                Err(error) => {
                    // Nothing was used up; the saved record is never
                    // republished (see `republish_commits`), and the
                    // admin's repair replaces it.
                    state.record_epoch(&admin)?;
                    return Err(error.context(format!("ignoring invalid commit for epoch {epoch}")));
                }
            };
            state.record_epoch(&admin)?;
            outcome
        };
        self.forget_skipped_segments();
        if outcome.removed_self {
            self.mark_removed();
            self.emit(Change::Removed);
            return Ok(CommitOutcome::RemovedSelf);
        }
        self.emit(Change::Members);
        Ok(CommitOutcome::MovedOn)
    }

    /// Does what freezing removed members' logs left for the views, and
    /// tells the screens if they changed. Retried at every pass until done.
    pub(crate) fn finish_view_work(&self) -> Result<()> {
        let is_changed = self.state().store.finish_view_work(self.apply_context())?;
        if is_changed {
            self.emit_all_views_changed()?;
        }
        Ok(())
    }

    /// Checks the commit's authenticated data against the wrapper's lists,
    /// before MLS sees the commit: staging it would use it up, and the right
    /// wrapper may come later. A commit that binds nothing is only taken from
    /// an admin's app that never binds (0.1.4), until this device has
    /// followed, or joined through, a bound one.
    fn check_binding(&self, epoch: u64, verified: &commit::VerifiedCommit) -> Result<()> {
        let binding = Mls::commit_binding(&verified.commit)?;
        let state = self.state();
        ensure!(state.mls.epoch()? == epoch, "the commit is for another epoch");
        if binding.is_empty() {
            ensure!(
                state.store.meta(BOUND_COMMIT_FOLLOWED_META)?.is_none(),
                "the commit doesn't bind its lists, as the admin's commits now do"
            );
            return Ok(());
        }
        let expected = commit::lists_hash(&state.mls.epoch_key()?, epoch, &verified.frozen, &[])?;
        ensure!(binding == expected, "the commit's lists don't match the commit");
        Ok(())
    }

    /// After joining: if the commit that added this device binds its lists,
    /// commits that bind nothing are junk from now on. This takes the
    /// admin's app as it was when we joined.
    pub(crate) async fn note_a_bound_join(&self) -> Result<()> {
        let admin = self.inner.config.admin;
        let join_epoch = self.state().mls.epoch()?;
        let from_epoch = join_epoch.checked_sub(1).context("joined at the first epoch")?;
        let bytes = self
            .bucket()
            .commit(from_epoch)
            .await?
            .context("the commit that added this device isn't in the bucket")?;
        let verified = commit::verify(&bytes, from_epoch, &admin)?;
        let binding = Mls::commit_binding(&verified.commit)?;
        if binding.is_empty() {
            return Ok(());
        }
        let state = self.state();
        let key = state
            .store
            .epoch_key(from_epoch)?
            .context("the grant has no key for the epoch before joining")?;
        let expected = commit::lists_hash(&key, from_epoch, &verified.frozen, &[])?;
        ensure!(
            binding == expected,
            "the commit that added this device doesn't match its lists"
        );
        state.store.set_meta(BOUND_COMMIT_FOLLOWED_META, &[1])
    }

    /// Re-uploads any admin commit we know of that is missing or altered in
    /// the bucket. Every member does this, so deleting commit slots (to make
    /// a stale recovery snapshot look current) gets undone. Commits are
    /// admin-signed, so anyone may re-upload them.
    /// One listing tells which slots changed since we last found them
    /// right; only those are fetched and compared. A slot that can't be read
    /// is written over: anyone with the bucket keys can put an object there
    /// that a plain GET is refused, to keep members from following.
    pub(crate) async fn republish_commits(&self) -> Result<()> {
        // Only commits this device has left behind: a saved one that MLS then
        // refused must never be written back over the admin's repair.
        let records = {
            let state = self.state();
            let current = state.mls.epoch()?;
            let mut records = state.store.commit_records()?;
            records.retain(|(from_epoch, _)| *from_epoch < current);
            records
        };
        let listed: std::collections::HashMap<u64, String> =
            self.bucket().listed_commits().await?.into_iter().collect();
        for (from_epoch, signed) in records {
            let listed_version = listed.get(&from_epoch);
            let verified = self.verified_commits().get(&from_epoch).cloned();
            if listed_version.is_some() && listed_version == verified.as_ref() {
                continue;
            }
            // Verified at the version the bytes were read at, not the one
            // listed: the slot may change in between.
            let fetched = match self.bucket().commit_versioned(from_epoch).await {
                Ok(fetched) => fetched,
                Err(error) => {
                    tracing::warn!(
                        "commit slot {from_epoch} can't be read, so it is written again: {error:#}"
                    );
                    None
                }
            };
            match fetched {
                Some((bytes, version)) if bytes.as_ref() == signed.as_slice() => {
                    self.verified_commits().insert(from_epoch, version);
                }
                _ => self.bucket().put_commit(from_epoch, signed).await?,
            }
        }
        Ok(())
    }

    fn verified_commits(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<u64, String>> {
        self.inner
            .verified_commits
            .lock()
            .expect("verified commits lock poisoned by an earlier panic")
    }

    fn emit_all_views_changed(&self) -> Result<()> {
        let changes = self.state().store.all_view_changes()?;
        for change in changes {
            self.emit(change);
        }
        Ok(())
    }
}
