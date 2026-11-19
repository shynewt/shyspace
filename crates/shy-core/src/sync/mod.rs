//! Keeps this member in step with the others: a background loop that follows
//! membership changes, pulls and pushes ops through the bucket, and applies
//! ops arriving live over gossip.

mod alerts;
mod receive;
mod replicate;

use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc;
use tracing::warn;

use crate::{ops::now_ms, transport::gossip::Wire, views::Change, workspace::Workspace};

const SYNC_INTERVAL: Duration = Duration::from_secs(8);
/// Batches bursts of writes (like typing) into one bucket upload.
const PUSH_DEBOUNCE: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    pub peers: usize,
    pub last_sync_ms: Option<i64>,
    pub error: Option<String>,
    pub removed: bool,
    /// A teammate's newer version sent a change this one can't read, and
    /// their later changes wait for it until this app is updated.
    pub needs_update: bool,
}

impl Workspace {
    pub(crate) fn spawn_background(&self, mut incoming: mpsc::Receiver<Wire>) {
        let receiver = self.clone();
        tokio::spawn(async move {
            while let Some(wire) = incoming.recv().await {
                match wire {
                    Wire::Envelope(envelope) => {
                        if let Err(error) = receiver.receive(envelope) {
                            warn!("dropping live op: {error:#}");
                        }
                    }
                    Wire::Nudge(_) => receiver.inner.wake.notify_one(),
                }
            }
        });

        let syncer = self.clone();
        tokio::spawn(async move {
            syncer.announce_self().await;
            loop {
                syncer.sync().await;
                if syncer.status().removed {
                    break;
                }
                tokio::select! {
                    _ = syncer.inner.wake.notified() => tokio::time::sleep(PUSH_DEBOUNCE).await,
                    _ = tokio::time::sleep(SYNC_INTERVAL) => {}
                }
            }
        });
    }

    pub fn status(&self) -> SyncStatus {
        let mut status = self.status_lock().clone();
        status.peers = self.inner.net.as_ref().map_or(0, |net| net.neighbors());
        status
    }

    fn status_lock(&self) -> std::sync::MutexGuard<'_, SyncStatus> {
        self.inner
            .status
            .lock()
            .expect("sync status lock poisoned by an earlier panic")
    }

    pub(crate) fn mark_removed(&self) {
        self.status_lock().removed = true;
    }

    /// One full pass. Each step runs even if an earlier one failed, so one
    /// bad object can't stop the others; the first error is shown in the UI.
    pub async fn sync(&self) {
        let _guard = self.inner.sync_lock.lock().await;
        let mut errors = Vec::new();
        if let Err(error) = self.follow_admin_commits().await {
            errors.push(error);
        }
        match self.apply_pending() {
            Ok(needs_update) => self.status_lock().needs_update = needs_update,
            Err(error) => errors.push(error),
        }
        if !self.status().removed {
            // The admin repairs commits as part of its duties, after its
            // recovery snapshot; members repair them here.
            let repaired = if self.is_admin() {
                self.admin_duties().await
            } else {
                self.republish_commits().await
            };
            if let Err(error) = repaired {
                errors.push(error);
            }
            for result in [self.pull().await, self.push().await, self.join_peers().await] {
                if let Err(error) = result {
                    errors.push(error);
                }
            }
        }
        {
            let mut status = self.status_lock();
            match (errors.first(), now_ms()) {
                (None, Ok(now)) => {
                    status.last_sync_ms = Some(now);
                    status.error = None;
                }
                (Some(error), _) => status.error = Some(format!("{error:#}")),
                (None, Err(error)) => status.error = Some(format!("{error:#}")),
            }
        }
        self.emit(Change::Sync);
    }
}
