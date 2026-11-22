//! Keeps this member in step with the others: a background loop that follows
//! membership changes, pulls and pushes ops through the bucket, and applies
//! ops arriving live over gossip.

mod alerts;
mod forks;
mod peers;
mod receive;
mod replicate;

use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    ops::now_ms,
    transport::gossip::{Incoming, Wire},
    views::Change,
    views::PresenceState,
    workspace::{NextBeat, Workspace},
};

const SYNC_INTERVAL: Duration = Duration::from_secs(8);
/// Well under the presence timeout, so one lost heartbeat doesn't count.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);
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
    pub(crate) fn spawn_background(&self, mut incoming: mpsc::Receiver<Incoming>) {
        let receiver = self.clone();
        let mut stopped = self.inner.stopped.subscribe();
        tokio::spawn(async move {
            loop {
                let wire = tokio::select! {
                    wire = incoming.recv() => wire,
                    _ = stopped.changed() => None,
                };
                let Some(wire) = wire else { break };
                match wire {
                    Incoming::NeighborUp => {
                        receiver.inner.wake.notify_one();
                        receiver.inner.heartbeat_now.notify_one();
                    }
                    Incoming::Lagged => receiver.inner.wake.notify_one(),
                    Incoming::Message(Wire::Envelope(envelope)) => {
                        if let Err(error) = receiver.receive(envelope) {
                            warn!("couldn't take a live op: {error:#}");
                        }
                    }
                    Incoming::Message(Wire::Nudge(_)) => receiver.inner.wake.notify_one(),
                    Incoming::Message(Wire::Presence(heartbeat)) => {
                        match receiver.receive_heartbeat(&heartbeat) {
                            Ok(true) => receiver.emit(Change::Members),
                            Ok(false) => {}
                            Err(error) => warn!("ignoring a heartbeat: {error:#}"),
                        }
                    }
                }
            }
        });

        let beater = self.clone();
        let mut stopped = self.inner.stopped.subscribe();
        tokio::spawn(async move {
            let mut reachable = Vec::new();
            loop {
                let wait = match beater.send_heartbeat().await {
                    Ok(held_back) => held_back.unwrap_or(HEARTBEAT_INTERVAL),
                    Err(error) => {
                        warn!("could not send a heartbeat: {error:#}");
                        HEARTBEAT_INTERVAL
                    }
                };
                tokio::select! {
                    _ = beater.inner.heartbeat_now.notified() => {}
                    _ = tokio::time::sleep(wait) => {}
                    _ = stopped.changed() => break,
                }
                if beater.status().removed {
                    break;
                }
                // Someone whose heartbeats stopped is now unreachable, or
                // our own presence changed.
                match beater.reachable_members() {
                    Ok(now_reachable) if now_reachable != reachable => {
                        reachable = now_reachable;
                        beater.emit(Change::Members);
                    }
                    Ok(_) => {}
                    Err(error) => warn!("could not check who is reachable: {error:#}"),
                }
            }
        });

        let syncer = self.clone();
        let mut stopped = self.inner.stopped.subscribe();
        tokio::spawn(async move {
            syncer.announce_self().await;
            let mut last_announced = tokio::time::Instant::now();
            loop {
                // Staying announced keeps others dialing us while we're online.
                if last_announced.elapsed() >= Duration::from_millis(peers::ANNOUNCE_EVERY_MS.unsigned_abs())
                {
                    syncer.announce_self().await;
                    last_announced = tokio::time::Instant::now();
                }
                syncer.sync().await;
                if syncer.status().removed {
                    break;
                }
                tokio::select! {
                    _ = syncer.inner.wake.notified() => tokio::time::sleep(PUSH_DEBOUNCE).await,
                    _ = tokio::time::sleep(SYNC_INTERVAL) => {}
                    _ = stopped.changed() => break,
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
            Ok(pending) => {
                self.status_lock().needs_update = pending.is_waiting_for_update;
                errors.extend(pending.local_failure);
            }
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
            // Forks first, so ops proved forked never apply or alert here.
            let shared_forks = self.share_forks().await;
            let pulled = self.pull().await;
            let tended = self.tend_private_channels();
            for result in [
                shared_forks,
                pulled,
                tended,
                self.push().await,
                // After the push, so a delete is out before its file goes.
                self.remove_unused_blobs().await,
                self.join_peers().await,
            ] {
                if let Err(error) = result {
                    errors.push(error);
                }
            }
            // Not a sync error: syncing works either way, and the app shows
            // the result on its own.
            if let Err(error) = self.check_storage_protection_when_due().await {
                warn!("couldn't check whether the bucket keeps deleted data: {error:#}");
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

    /// Returns how soon to try again when the beat was held back.
    async fn send_heartbeat(&self) -> anyhow::Result<Option<Duration>> {
        let Some(net) = &self.inner.net else {
            return Ok(None);
        };
        match self.next_heartbeat()? {
            NextBeat::Now(heartbeat) => net.broadcast_presence(heartbeat).await.map(|()| None),
            NextBeat::After(wait) => Ok(Some(wait)),
            NextBeat::Silent => Ok(None),
        }
    }

    fn reachable_members(&self) -> anyhow::Result<Vec<(iroh::PublicKey, PresenceState)>> {
        let now = now_ms()?;
        Ok(self
            .current_members()?
            .into_iter()
            .map(|member| (member, self.presence_of(&member, now)))
            .collect())
    }
}
