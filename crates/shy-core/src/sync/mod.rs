//! Keeps this member in step with the others: a background loop that follows
//! membership changes, pulls and pushes ops through the bucket, and applies
//! ops arriving live over gossip.

mod alerts;
mod forks;
mod gaps;
mod heads;
mod pacing;
pub use pacing::Cadence;
mod packing;
mod peers;
mod receive;
pub(crate) mod relay;
mod replicate;

pub use heads::Hint;
pub use relay::RelayState;

use std::time::Duration;

/// How often huddle participants whose refreshes stopped are dropped.
const HUDDLE_EXPIRY_CHECK: Duration = Duration::from_secs(5);

use serde::Serialize;
use tokio::sync::mpsc;
use tracing::warn;

use pacing::{Pacing, SHORT_WAIT, has_slept};

use crate::{
    ops::now_ms,
    transport::{
        direct::DirectMessage,
        gossip::{Incoming, Wire},
    },
    views::Change,
    views::{PresenceState, member_id},
    workspace::{NextBeat, Via, Workspace},
};

/// Well under the presence timeout, so one lost heartbeat doesn't count.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);
/// How often commit slots, fork proofs and peers are looked at between sweeps.
const SLOW_DUTIES_EVERY: Duration = Duration::from_secs(10 * 60);
/// Batches bursts of writes (like typing) into one bucket upload.
const PUSH_DEBOUNCE: Duration = Duration::from_millis(600);

pub(crate) enum NextCheck {
    Due,
    /// Something happened: check now, and start from the short wait again.
    Woken,
    /// The computer slept: sweep now.
    Slept,
    Stopped,
}

#[derive(Clone, Copy)]
enum PassKind {
    Sweep,
    Check,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    pub peers: usize,
    pub last_sync_ms: Option<i64>,
    pub error: Option<String>,
    pub removed: bool,
    /// A teammate's newer version sent a change this one can't read, and
    /// their later changes wait for it until this app is updated.
    pub needs_update: bool,
    /// Whose apps sent those changes (member ids), so a false alarm (junk
    /// signed on purpose) points at its source.
    pub needs_update_from: Vec<String>,
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
                    Incoming::Message(Wire::Hint(hint)) => match receiver.receive_hint(&hint) {
                        Ok(true) => receiver.inner.wake.notify_one(),
                        Ok(false) => {}
                        Err(error) => warn!("ignoring a hint: {error:#}"),
                    },
                    Incoming::Message(Wire::Huddle(beacon)) => {
                        if let Err(error) = receiver.receive_beacon(&beacon, Via::Gossip) {
                            warn!("ignoring a huddle beacon: {error:#}");
                        }
                    }
                    Incoming::Direct {
                        from,
                        message: DirectMessage::Huddle(beacon),
                    } => {
                        if let Err(error) = receiver.receive_beacon(&beacon, Via::Direct(from)) {
                            warn!("ignoring a direct huddle beacon: {error:#}");
                        }
                    }
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

        let expirer = self.clone();
        let mut stopped = self.inner.stopped.subscribe();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(HUDDLE_EXPIRY_CHECK);
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = stopped.changed() => break,
                }
                if let Err(error) = expirer.expire_huddle_participants() {
                    warn!("couldn't expire huddle participants: {error:#}");
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

        // What we write goes up on its own, not behind a long pull.
        let pusher = self.clone();
        let mut push_stopped = self.inner.stopped.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = pusher.inner.push_now.notified() => {}
                    _ = push_stopped.changed() => break,
                }
                tokio::select! {
                    _ = tokio::time::sleep(PUSH_DEBOUNCE) => {}
                    _ = push_stopped.changed() => break,
                }
                if let Err(error) = pusher.push().await {
                    warn!("couldn't upload our changes yet; the next check tries again: {error:#}")
                }
            }
        });

        let syncer = self.clone();
        let mut stopped = self.inner.stopped.subscribe();
        tokio::spawn(async move {
            syncer.announce_self().await;
            let mut last_announced = tokio::time::Instant::now();
            let mut pacing = Pacing::default();
            // A sweep at start: things may have moved while we were away.
            let mut is_sweep_due = true;
            loop {
                // Staying announced keeps others dialing us while we're online.
                if last_announced.elapsed() >= Duration::from_millis(peers::ANNOUNCE_EVERY_MS.unsigned_abs())
                {
                    syncer.announce_self().await;
                    last_announced = tokio::time::Instant::now();
                }
                let is_hour_up = syncer.is_sweep_due().unwrap_or_else(|error| {
                    warn!("couldn't tell when we last swept: {error:#}");
                    true
                });
                let kind = match is_sweep_due || is_hour_up {
                    true => PassKind::Sweep,
                    false => PassKind::Check,
                };
                let has_found_new = syncer.pass(kind).await;
                if syncer.status().removed {
                    break;
                }
                let wait = syncer.cadence().stretch(pacing.after_check(has_found_new));
                is_sweep_due = false;
                match syncer.wait_for_next_check(wait, &mut stopped).await {
                    NextCheck::Due => {}
                    NextCheck::Woken => pacing.wake(),
                    NextCheck::Slept => {
                        pacing.wake();
                        is_sweep_due = true;
                    }
                    NextCheck::Stopped => break,
                }
            }
        });
    }

    /// Waits `wait`, looking every `SHORT_WAIT` for reasons to check sooner:
    /// a wake (a nudge, a hint naming ops we lack), the computer having slept
    /// (its neighbours may be gone and the bucket moved on), or a join request
    /// (joiners aren't members, so they can't nudge us).
    pub(crate) async fn wait_for_next_check(
        &self,
        wait: Duration,
        stopped: &mut tokio::sync::watch::Receiver<bool>,
    ) -> NextCheck {
        let started = tokio::time::Instant::now();
        let started_wall = std::time::SystemTime::now();
        loop {
            let remaining = wait.saturating_sub(started.elapsed());
            tokio::select! {
                _ = self.inner.wake.notified() => {
                    tokio::time::sleep(PUSH_DEBOUNCE).await;
                    // After a resume, a neighbour coming back often wakes us
                    // first; it still counts as having slept.
                    let wall_elapsed = started_wall.elapsed().unwrap_or_default();
                    if has_slept(wall_elapsed, started.elapsed()) {
                        return NextCheck::Slept;
                    }
                    return NextCheck::Woken;
                }
                _ = tokio::time::sleep(remaining.min(SHORT_WAIT)) => {}
                _ = stopped.changed() => return NextCheck::Stopped,
            }
            if started.elapsed() >= wait {
                return NextCheck::Due;
            }
            let wall_elapsed = started_wall.elapsed().unwrap_or_default();
            if has_slept(wall_elapsed, started.elapsed()) {
                return NextCheck::Slept;
            }
            if self.found_join_request().await {
                return NextCheck::Woken;
            }
        }
    }

    /// Reads only the open invites' request slots, so a joiner isn't kept
    /// waiting for the admin's next check.
    async fn found_join_request(&self) -> bool {
        if !self.is_admin() {
            return false;
        }
        let _guard = self.inner.sync_lock.lock().await;
        self.collect_join_requests().await.unwrap_or_else(|error| {
            warn!("couldn't look for join requests: {error:#}");
            false
        })
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

    /// Whether commits, fork proofs and peers are due (every 10 minutes);
    /// if so, starts the next interval.
    fn take_slow_duties_if_due(&self) -> bool {
        let mut last = self
            .inner
            .last_slow_duties
            .lock()
            .expect("slow duties lock poisoned by an earlier panic");
        let is_due = last.is_none_or(|last| last.elapsed() >= SLOW_DUTIES_EVERY);
        if is_due {
            *last = Some(tokio::time::Instant::now());
        }
        is_due
    }

    #[cfg(test)]
    pub(crate) fn make_slow_duties_due(&self) {
        *self
            .inner
            .last_slow_duties
            .lock()
            .expect("slow duties lock poisoned by an earlier panic") = None;
    }

    /// Runs this workspace live or in the background. Going live checks at once.
    pub fn set_cadence(&self, cadence: Cadence) {
        let previous = std::mem::replace(&mut *self.cadence_lock(), cadence);
        if previous != cadence && cadence == Cadence::Live {
            self.inner.wake.notify_one();
        }
    }

    pub fn cadence(&self) -> Cadence {
        *self.cadence_lock()
    }

    fn cadence_lock(&self) -> std::sync::MutexGuard<'_, Cadence> {
        self.inner
            .cadence
            .lock()
            .expect("cadence lock poisoned by an earlier panic")
    }

    pub(crate) fn mark_removed(&self) {
        self.status_lock().removed = true;
    }

    /// One full pass (a sweep): every author's segments past what we have
    /// checked against the bucket. Each step runs even if an earlier one
    /// failed, so one bad object can't stop the others; the first error is
    /// shown in the UI.
    pub async fn sync(&self) {
        self.pass(PassKind::Sweep).await;
    }

    /// A light pass: reads only the logs whose head moved in a way gossip
    /// didn't already explain. Returns whether it brought ops gossip missed.
    #[cfg(test)]
    pub(crate) async fn check(&self) -> bool {
        self.pass(PassKind::Check).await
    }

    async fn pass(&self, kind: PassKind) -> bool {
        let _guard = self.inner.sync_lock.lock().await;
        let mut errors = Vec::new();
        let mut has_found_new = false;
        if let Err(error) = self.follow_admin_commits().await {
            errors.push(error);
        }
        match self.apply_pending() {
            Ok(pending) => {
                let mut status = self.status_lock();
                status.needs_update = !pending.waiting_for_update.is_empty();
                status.needs_update_from = pending.waiting_for_update.iter().map(member_id).collect();
                drop(status);
                errors.extend(pending.local_failure);
            }
            Err(error) => errors.push(error),
        }
        if !self.status().removed {
            let is_slow_due = matches!(kind, PassKind::Sweep) || self.take_slow_duties_if_due();
            let mut has_snapshot = true;
            if self.is_admin() {
                if let Err(error) = self.refresh_recovery_snapshot().await {
                    errors.push(error);
                    has_snapshot = false;
                }
                if let Err(error) = self.answer_join_requests().await {
                    errors.push(error);
                }
                if is_slow_due && let Err(error) = self.recheck_old_key_if_unknown().await {
                    errors.push(error);
                }
            }
            // Commit repair only once the admin's recovery snapshot is up (a
            // commit must never be in the bucket before the snapshot that
            // covers it), and fork proofs before reading, so ops proved
            // forked never apply.
            let repaired = match is_slow_due && has_snapshot {
                true => self.republish_commits().await,
                false => Ok(()),
            };
            let shared_forks = match is_slow_due {
                true => self.share_forks().await,
                false => Ok(()),
            };
            if let Err(error) = repaired {
                errors.push(error);
            }
            let pulled = match kind {
                PassKind::Sweep => self.pull().await,
                PassKind::Check => self.check_heads().await,
            }
            .map(|found| has_found_new = found);
            let tended = self.tend_private_channels();
            for result in [
                shared_forks,
                pulled,
                tended,
                self.push().await,
                self.relay_live_only_ops().await,
                // After the push, so a delete is out before its file goes.
                self.remove_unused_blobs().await,
                self.refresh_gossip_members(),
                match is_slow_due {
                    true => self.join_peers().await,
                    false => Ok(()),
                },
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
        has_found_new
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
