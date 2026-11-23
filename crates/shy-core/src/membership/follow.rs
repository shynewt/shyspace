//! Members apply the admin's commits, in epoch order, as they appear.

use anyhow::{Context, Result};

use super::commit;
use crate::{views::Change, workspace::Workspace};

impl Workspace {
    pub(crate) async fn follow_admin_commits(&self) -> Result<()> {
        let admin = self.inner.config.admin;
        if self.me() == admin {
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
            let verified = commit::verify(&bytes, epoch, &admin)
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
                state.store.put_commit_record(epoch, &bytes)?;
                state.record_epoch()?;
                outcome
            };
            if outcome.removed_self {
                self.mark_removed();
                self.emit(Change::Removed);
                return Ok(());
            }
            self.emit(Change::Members);
        }
    }

    /// Re-uploads any admin commit we know of that is missing or altered in
    /// the bucket. Every member does this, so deleting commit slots (to make
    /// a stale recovery snapshot look current) gets undone. Commits are
    /// admin-signed, so anyone may re-upload them.
    /// One listing tells which slots changed since we last found them
    /// right; only those are fetched and compared.
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
            if self.bucket().commit(from_epoch).await?.as_deref() == Some(signed.as_slice()) {
                if let Some(version) = listed_version {
                    self.verified_commits().insert(from_epoch, version.clone());
                }
            } else {
                self.bucket().put_commit(from_epoch, signed).await?;
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
