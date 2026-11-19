//! Members apply the admin's commits, in epoch order, as they appear.

use anyhow::{Context, Result};

use super::commit::SignedCommit;
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
            let commit = SignedCommit::verify(&bytes, epoch, &admin)
                .with_context(|| format!("ignoring invalid commit for epoch {epoch}"))?;
            let outcome = {
                let mut state = self.state();
                let outcome = state.mls.process_commit(&commit, &admin)?;
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
}
