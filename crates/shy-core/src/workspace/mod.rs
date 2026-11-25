//! The public entry point: one workspace, stored in one data directory.

mod actions;
mod alert_actions;
mod files;
mod huddle_ring;
mod huddles;
mod inbox;
mod key_rotation;
mod later;
mod lifecycle;
mod look;
mod page_mentions;
mod pages_live;
mod presence;
mod protection;
mod reads;
mod sections;
mod sent;
mod storage;

pub(crate) use actions::OpMaker;
pub use files::{FileSource, MAX_FILES_PER_MESSAGE, NewFile};
pub use huddles::Via;
#[cfg(test)]
pub(crate) use lifecycle::CUT_SHORT_CREATES;
pub use lifecycle::{DataDir, JoinCancel};
pub(crate) use lifecycle::{MLS_FILE, STORE_FILE};
pub use presence::Heartbeat;
pub(crate) use presence::NextBeat;
pub(crate) use reads::follows_by_default;
pub use storage::StorageUpdate;

use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use anyhow::{Context, Result};
use iroh::{PublicKey, SecretKey};
use tokio::sync::{Notify, broadcast};

use crate::{
    config::WorkspaceConfig,
    device_settings::DeviceSettingsFile,
    huddle::media::HuddleMedia,
    keys::DeviceKey,
    membership::{admin::drop_unsealed_deliveries, mls::Mls},
    storage::Store,
    sync::SyncStatus,
    transport::{
        bucket::{Bucket, Connector},
        gossip::{ExtraProtocols, Net},
    },
    views::{Change, member_id},
};

/// Whether to go online. Tests run several members against one in-memory
/// bucket without networking.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Networking {
    Online,
    Offline,
}

/// How to open a workspace: whether to go online, what else to serve on its
/// endpoint, and what moves huddle audio. A plain [`Networking`] converts
/// into options with nothing extra.
#[derive(Clone)]
pub struct WorkspaceOptions {
    pub networking: Networking,
    pub protocols: Option<ExtraProtocols>,
    /// Without one, the workspace can show huddles but not join them.
    pub media: Option<Arc<dyn HuddleMedia>>,
    /// The device's settings, where its huddle devices are kept; without
    /// them (a workspace opened on its own) the workspace keeps its own.
    pub(crate) device_settings: Option<Arc<DeviceSettingsFile>>,
}

impl WorkspaceOptions {
    pub fn with_protocols(mut self, protocols: ExtraProtocols) -> Self {
        self.protocols = Some(protocols);
        self
    }

    pub fn with_media(mut self, media: Arc<dyn HuddleMedia>) -> Self {
        self.media = Some(media);
        self
    }
}

impl From<Networking> for WorkspaceOptions {
    fn from(networking: Networking) -> Self {
        Self {
            networking,
            protocols: None,
            media: None,
            device_settings: None,
        }
    }
}

pub struct Profile<'a> {
    pub name: &'a str,
    pub is_bot: bool,
}

/// Everything that changes together under one lock: the database and the
/// MLS group must never disagree about the current epoch.
pub(crate) struct State {
    pub store: Store,
    pub mls: Mls,
}

impl State {
    /// After a crash, finishes a staged commit only if it was signed and
    /// saved (so it can reach the bucket); otherwise forgets it.
    fn recover_staged_commit(&mut self) -> Result<()> {
        if !self.mls.has_pending_commit() {
            return Ok(());
        }
        if self.store.has_commit_record(self.mls.epoch()?)? {
            self.mls.merge_pending_commit()?;
        } else {
            self.mls.discard_pending_commit()?;
            drop_unsealed_deliveries(&self.store)?;
        }
        Ok(())
    }

    /// Stores the current epoch's key and member list after any epoch change.
    pub fn record_epoch(&self) -> Result<()> {
        if self.mls.is_active() {
            let members = self.mls.members()?;
            self.store
                .put_epoch(self.mls.epoch()?, &self.mls.epoch_key()?, &members)?;
            self.store.set_current_members(&members)?;
        }
        Ok(())
    }
}

pub(crate) struct Inner {
    pub data: DataDir,
    /// Never read: holding it keeps the data folder locked until the
    /// workspace is dropped. See `DataDir::lock`.
    pub _data_lock: std::fs::File,
    /// Kept for the admin's recovery snapshots, which carry it.
    pub device_key: DeviceKey,
    pub identity: SecretKey,
    pub config: WorkspaceConfig,
    pub state: Mutex<State>,
    /// Swapped when the admin changes the storage settings.
    pub bucket: RwLock<Bucket>,
    pub connect: Connector,
    pub net: Option<Net>,
    pub huddle_media: Option<Arc<dyn HuddleMedia>>,
    pub(crate) device_settings: Option<Arc<DeviceSettingsFile>>,
    pub events: broadcast::Sender<Change>,
    /// Wakes the sync loop early: after a local write or a peer's nudge.
    pub wake: Notify,
    pub sync_lock: tokio::sync::Mutex<()>,
    /// Starts an upload of our new ops, apart from the sync pass, so a long
    /// catch-up doesn't hold back what we write.
    pub push_now: Notify,
    /// One upload at a time, so the same ops never go up twice at once.
    pub push_lock: tokio::sync::Mutex<()>,
    /// When each pending join answer last went up (by a clock that never
    /// goes back), so it goes up again only now and then rather than on
    /// every pass; in memory only.
    pub answers_sent: Mutex<std::collections::HashMap<iroh::PublicKey, tokio::time::Instant>>,
    pub recovery_lock: tokio::sync::Mutex<()>,
    pub status: Mutex<SyncStatus>,
    /// Gaps (author, missing seq) already fetched again this run, so a gap
    /// the bucket can't fill doesn't make every pass re-read everything.
    pub refetched_gaps: Mutex<std::collections::HashSet<(iroh::PublicKey, u64)>>,
    /// Objects among someone's segments that reading can't resume after
    /// (junk, or ops that didn't apply), already read this run in this
    /// version: not fetched again unless replaced, so junk can't cost a
    /// download on every pass.
    pub skipped_segments: Mutex<std::collections::HashSet<crate::transport::bucket::ListedObject>>,
    /// How often each object among someone's segments or the fork proofs
    /// failed to download this run; one that keeps failing is given up.
    pub fetch_failures: Mutex<std::collections::HashMap<crate::transport::bucket::ListedObject, u32>>,
    /// When commits, fork proofs and peers were last looked at; they need
    /// it every 10 minutes, not every check.
    pub last_slow_duties: Mutex<Option<tokio::time::Instant>>,
    /// The version of each commit slot we last found right.
    pub verified_commits: Mutex<std::collections::HashMap<u64, String>>,
    /// What we held of each unfinished removed member's log when we last
    /// read it, so a check reads again only once something new came.
    pub removal_reads: Mutex<std::collections::HashMap<PublicKey, u64>>,
    /// The head version we last acted on, per author; in memory only.
    pub seen_heads: Mutex<std::collections::HashMap<PublicKey, String>>,
    /// The seq our own head was last confirmed to hold.
    pub confirmed_head_seq: std::sync::atomic::AtomicU64,
    /// The latest signed hint per author: (seq, head version).
    pub heard_hints: Mutex<std::collections::HashMap<PublicKey, (u64, String)>>,
    /// Live, or checking the bucket only every few minutes in the background.
    pub cadence: Mutex<crate::sync::Cadence>,
    /// What each author has in the bucket, for relaying ops that never got there.
    pub relays: Mutex<crate::sync::RelayState>,
    /// Fork proof objects in the bucket already looked at, by path, with the
    /// version we saw: our own found intact, and ones found overwritten.
    pub seen_fork_proofs: Mutex<std::collections::HashMap<String, String>>,
    /// When we last dialed each peer over gossip, by our clock.
    pub last_dialed_ms: Mutex<std::collections::HashMap<iroh::PublicKey, i64>>,
    /// Whom we asked for the storage settings since our key stopped
    /// working; in memory only.
    pub key_round: Mutex<crate::sync::key_handoff::KeyRound>,
    /// When we last answered each member asking for the storage settings.
    pub key_answers: Mutex<std::collections::HashMap<PublicKey, tokio::time::Instant>>,
    /// Others' presence, from heartbeats; in memory only.
    pub presence: Mutex<presence::PresenceBook>,
    pub huddles: Mutex<huddles::HuddleState>,
    pub huddle_transition: Mutex<()>,
    /// The runtime the workspace was opened on, so its sync calls can start
    /// tasks from any thread.
    pub runtime: tokio::runtime::Handle,
    pub page_live: Mutex<pages_live::PageLive>,
    pub page_alert_times: Mutex<crate::sync::page_mentions::PageAlertTimes>,
    #[cfg(test)]
    pub page_sent: Mutex<Vec<crate::transport::page_gossip::PageWire>>,
    pub own_presence: Mutex<presence::OwnPresence>,
    /// Sends a heartbeat now instead of at the next tick.
    pub heartbeat_now: Notify,
    /// How much the file cache holds, so trimming needn't count it each time.
    pub file_cache_size: crate::files::cache::CacheSize,
    /// Set by `shutdown`; the background loops end when they see it.
    pub stopped: tokio::sync::watch::Sender<bool>,
    /// Makes the next apply fail as a full disk would.
    #[cfg(test)]
    pub fail_next_apply: std::sync::atomic::AtomicBool,
    /// Makes the next parking of an op fail as a full disk would.
    #[cfg(test)]
    pub fail_next_park: std::sync::atomic::AtomicBool,
    /// Makes the next downloaded segment fail just before it is saved, as if
    /// the app died then.
    #[cfg(test)]
    pub fail_segment_save: std::sync::atomic::AtomicBool,
    /// Makes SQLite give up the next downloaded segment's transaction while
    /// it is applying an op, as a full disk can.
    #[cfg(test)]
    pub drop_segment_transaction: std::sync::atomic::AtomicBool,
    /// Makes the next join answer upload fail as an unreachable bucket would.
    #[cfg(test)]
    pub fail_next_answer_upload: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub struct Workspace {
    pub(crate) inner: Arc<Inner>,
}

impl Workspace {
    pub(crate) fn apply_context(&self) -> crate::storage::ApplyContext<'_> {
        crate::storage::ApplyContext {
            workspace_id: &self.inner.config.workspace_id,
            admin: self.inner.config.admin,
            me: self.me(),
        }
    }

    /// Whether this member may make `op`, by the rule every member enforces.
    pub(crate) fn may(&self, op: &crate::ops::Op) -> bool {
        op.is_permitted(&self.me(), &self.inner.config.admin)
    }

    pub(crate) fn bucket(&self) -> Bucket {
        self.inner
            .bucket
            .read()
            .expect("bucket lock poisoned by an earlier panic")
            .clone()
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .expect("workspace state lock poisoned by an earlier panic")
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.inner.events.subscribe()
    }

    pub(crate) fn emit(&self, change: Change) {
        self.on_storage_changed(&change);
        // No subscribers is fine: nobody is looking at the views right now.
        let _ = self.inner.events.send(change);
    }

    pub fn me(&self) -> PublicKey {
        self.inner.identity.public()
    }

    pub fn my_id(&self) -> String {
        member_id(&self.me())
    }

    pub fn safety_code(&self) -> String {
        crate::views::safety_code(&self.me())
    }

    pub fn name(&self) -> &str {
        &self.inner.config.name
    }

    pub fn workspace_id(&self) -> &str {
        &self.inner.config.workspace_id
    }

    pub fn is_admin(&self) -> bool {
        self.inner.config.admin == self.me()
    }

    /// Current MLS members; empty once we've been removed.
    pub(crate) fn current_members(&self) -> Result<Vec<PublicKey>> {
        let state = self.state();
        if !state.mls.is_active() {
            return Ok(Vec::new());
        }
        state.mls.members()
    }

    /// Stops this workspace's background tasks at once, as `shutdown` does,
    /// without waiting for anything: for when shutting down took too long.
    pub fn stop_background(&self) {
        self.inner.stopped.send_replace(true);
    }

    /// Stops the background work, uploads what wasn't sent yet (so a message
    /// written just before quitting isn't held back until the next start),
    /// then closes the network.
    pub async fn shutdown(&self) -> Result<()> {
        let left = self.leave_huddle();
        self.finish_huddle_sends().await;
        self.inner.stopped.send_replace(true);
        self.close_page();
        let uploaded = self.push().await;
        if let Some(net) = &self.inner.net {
            net.shutdown().await;
        }
        uploaded.context("uploading unsent changes before closing")?;
        left.context("leaving the huddle before closing")
    }
}
