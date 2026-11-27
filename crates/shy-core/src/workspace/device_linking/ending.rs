//! How a link ends on the new device: joined, with the linking store let
//! go once nothing holds it open, or wiped, so nothing it was handed stays.

use std::sync::Arc;

#[cfg(test)]
use anyhow::bail;
use anyhow::{Context, Result};

use super::{DeviceLinking, LINK_IS_OVER};
use crate::views::Change;
#[cfg(test)]
use crate::workspace::DataDir;
use crate::{
    membership::{admin::DeviceWelcome, invite::JoinMailbox},
    transport::bucket::Connector,
    workspace::{
        Workspace, WorkspaceOptions,
        linking_store::{mark_link_ended, mark_link_finishing},
    },
};

impl<M: JoinMailbox> DeviceLinking<M> {
    /// Joins with `welcome` in the real folder, which starts with this
    /// device's drafts and read positions, then lets the linking store go.
    /// It is marked first: a join that stops partway can't run again.
    pub(super) async fn finish(
        mut self,
        reading: Workspace,
        welcome: DeviceWelcome,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        let enrollment = self.enrollment.take().context(LINK_IS_OVER)?;
        let kept = {
            let store = &reading.state().store;
            mark_link_finishing(store)?;
            store
                .personal_reading()
                .context("keeping what this device typed while it read")?
        };
        enrollment.keep_personal_reading(&kept)?;
        #[cfg(test)]
        if is_stopped_for_test(&STOPS_BEFORE_JOINING, &self.linking) {
            bail!("a simulated stop before joining");
        }
        let joined = enrollment.finish_with(welcome, connect, options).await;
        #[cfg(test)]
        if is_stopped_for_test(&STOPS_BEFORE_FORGETTING_READING, &self.linking) {
            return joined;
        }
        self.close_reading(reading).await?;
        joined
    }

    /// Stops linking this device, for good: both stores are wiped, so
    /// nothing it was handed stays. Never for shutting the app down:
    /// dropping the handle (quitting) keeps the link, and a restart resumes
    /// it.
    pub async fn cancel(mut self, reading: Option<Workspace>) -> Result<()> {
        self.wipe(reading).await
    }

    /// Wipes both stores, then returns `error`.
    pub(super) async fn wiped_after<T>(
        &mut self,
        error: anyhow::Error,
        reading: Option<Workspace>,
    ) -> Result<T> {
        self.wipe(reading).await?;
        Err(error)
    }

    /// Deletes both stores: the link is over and this device holds nothing.
    /// The link is marked ended first, so a stop partway leaves nothing a
    /// restart picks up again.
    async fn wipe(&mut self, reading: Option<Workspace>) -> Result<()> {
        if let Some(reading) = &reading {
            mark_link_ended(&reading.state().store)?;
        }
        if let Some(enrollment) = self.enrollment.take() {
            enrollment.forget().await?;
        }
        match reading {
            Some(reading) => self.close_reading(reading).await,
            None => self.linking.forget_reading().await,
        }
    }

    /// Lets the linking store go: now, if `reading` is its only handle, and
    /// otherwise once the last one closes. The app's other handles stop
    /// syncing and are told to let go; until they do, they can still read
    /// what the store holds on this device.
    async fn close_reading(&self, reading: Workspace) -> Result<()> {
        if Arc::strong_count(&reading.inner) == 1 {
            drop(reading);
            return self.linking.forget_reading().await;
        }
        reading.stop_background();
        reading.emit(Change::ReadingClosed);
        reading
            .inner
            .forget_on_close
            .forget_when_closed(self.linking.clone(), self.code.token);
        Ok(())
    }
}

/// Linking stores whose link stops, as if the app quit, after the join and
/// before the reading store is deleted.
#[cfg(test)]
pub(crate) static STOPS_BEFORE_FORGETTING_READING: std::sync::Mutex<Vec<std::path::PathBuf>> =
    std::sync::Mutex::new(Vec::new());

/// Linking stores whose link stops after marking the join as begun, before
/// it joins.
#[cfg(test)]
pub(crate) static STOPS_BEFORE_JOINING: std::sync::Mutex<Vec<std::path::PathBuf>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn is_stopped_for_test(stops: &std::sync::Mutex<Vec<std::path::PathBuf>>, linking: &DataDir) -> bool {
    stops
        .lock()
        .expect("simulated stops lock poisoned")
        .contains(&linking.path)
}
