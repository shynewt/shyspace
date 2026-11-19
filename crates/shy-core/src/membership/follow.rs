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
            let Some(bytes) = self.inner.bucket.commit(epoch).await? else {
                return Ok(());
            };
            let verified = commit::verify(&bytes, epoch, &admin)
                .with_context(|| format!("ignoring invalid commit for epoch {epoch}"))?;
            let outcome = {
                let mut state = self.state();
                let outcome = state.mls.process_commit(&verified.commit, &admin)?;
                state.store.put_commit_record(epoch, &bytes)?;
                for frozen in &verified.frozen {
                    state.store.freeze_author(&frozen.member, frozen.last_seq)?;
                }
                state.record_epoch()?;
                outcome
            };
            self.emit(Change::Members);
            if outcome.removed_self {
                self.mark_removed();
                self.emit(Change::Removed);
                return Ok(());
            }
        }
    }

    /// Re-uploads any admin commit we know of that is missing or altered in
    /// the bucket. Every member does this, so deleting commit slots (to make
    /// a stale recovery snapshot look current) gets undone. Commits are
    /// admin-signed, so anyone may re-upload them.
    pub(crate) async fn republish_commits(&self) -> Result<()> {
        let records = self.state().store.commit_records()?;
        for (from_epoch, signed) in records {
            if self.inner.bucket.commit(from_epoch).await?.as_deref() != Some(signed.as_slice()) {
                self.inner.bucket.put_commit(from_epoch, signed).await?;
            }
        }
        Ok(())
    }
}
