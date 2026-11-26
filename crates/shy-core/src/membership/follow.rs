//! Members apply the admin's commits, in epoch order, as they appear.

use anyhow::{Context, Result, ensure};

use super::{commit, mls::Mls};
use crate::{views::Change, workspace::Workspace};

/// Set once this device has followed a commit that binds its lists.
const BOUND_COMMIT_FOLLOWED_META: &str = "bound_commit_followed";

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
    pub(crate) fn apply_admin_commit(&self, epoch: u64, bytes: &[u8]) -> Result<CommitOutcome> {
        let admin = self.inner.config.admin;
        let verified = commit::verify(bytes, epoch, &admin)
            .with_context(|| format!("ignoring invalid commit for epoch {epoch}"))?;
        let is_bound = self
            .check_binding(epoch, &verified)
            .with_context(|| format!("ignoring invalid commit for epoch {epoch}"))?;
        // Frozen logs first: if undoing a removed member's late ops fails,
        // nothing has moved on and the whole commit is retried.
        let is_rebuilt = {
            let state = self.state();
            let mut is_rebuilt = false;
            for frozen in &verified.frozen {
                is_rebuilt |=
                    state
                        .store
                        .freeze_author(&frozen.member, frozen.last_seq, self.apply_context())?;
            }
            is_rebuilt
        };
        if is_rebuilt {
            self.emit_all_views_changed()?;
        }
        let outcome = {
            let mut state = self.state();
            let outcome = state.mls.process_commit(&verified.commit, &admin)?;
            state.store.put_commit_record(epoch, bytes)?;
            if is_bound {
                state.store.set_meta(BOUND_COMMIT_FOLLOWED_META, &[1])?;
            }
            state.record_epoch()?;
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

    /// Checks the commit's authenticated data against the wrapper's lists,
    /// before MLS sees the commit: staging it would use it up, and the right
    /// wrapper may come later. Returns whether the commit was bound. One that
    /// isn't is only taken from an admin's app that never binds (0.1.4),
    /// until a bound one has been followed.
    fn check_binding(&self, epoch: u64, verified: &commit::VerifiedCommit) -> Result<bool> {
        let binding = Mls::commit_binding(&verified.commit)?;
        let state = self.state();
        if binding.is_empty() {
            ensure!(
                state.store.meta(BOUND_COMMIT_FOLLOWED_META)?.is_none(),
                "the commit doesn't bind its lists, as the admin's commits now do"
            );
            return Ok(false);
        }
        let expected = commit::lists_hash(&state.mls.epoch_key()?, epoch, &verified.frozen, &[])?;
        ensure!(binding == expected, "the commit's lists don't match the commit");
        Ok(true)
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
        let records = self.state().store.commit_records()?;
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
