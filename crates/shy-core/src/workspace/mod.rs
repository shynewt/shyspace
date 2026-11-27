//! The public entry point: one workspace, stored in one data directory.

mod actions;
mod alert_actions;
pub(crate) mod device_linking;
mod drafts;
mod enrollment;
mod files;
mod huddle_ring;
mod huddles;
mod inbox;
mod key_rotation;
mod later;
mod lifecycle;
mod link_tending;
mod linking;
pub(crate) mod linking_store;
mod look;
mod page_mentions;
mod pages_live;
mod presence;
mod protection;
mod reading;
mod reads;
mod sections;
mod sent;
mod storage;

pub(crate) use actions::OpMaker;
pub use device_linking::{CheckOnNewDevice, DeviceLinking, LinkRefused, ResumedLink};
pub use enrollment::{DeviceEnrollment, DeviceLink, DeviceRequest};
pub use files::{FileSource, MAX_FILES_PER_MESSAGE, NewFile};
pub use huddles::Via;
#[cfg(test)]
pub(crate) use lifecycle::{CUT_SHORT_CLEARINGS, CUT_SHORT_CREATES};
pub use lifecycle::{DataDir, JoinCancel};
pub(crate) use lifecycle::{MLS_FILE, STORE_FILE};
pub use linking::LinkCheckShown;
pub use presence::Heartbeat;
pub(crate) use presence::NextBeat;
pub use reading::WorkspaceMode;
pub(crate) use reads::follows_by_default;
pub use storage::StorageUpdate;

use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, SecretKey};
use tokio::sync::{Notify, broadcast};

use crate::{
    config::WorkspaceConfig,
    device_settings::DeviceSettingsFile,
    huddle::media::HuddleMedia,
    keys::DeviceKey,
    membership::{
        admin::drop_unsealed_deliveries,
        commit::{self, FrozenAuthor, Wrapper},
        device::{BoundFreeze, map_digest},
        follow::BOUND_COMMIT_FOLLOWED_META,
        mls::Mls,
    },
    ops::{MAX_STORED_NUMBER, now_ms},
    storage::{DeviceSource, Store},
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
#[cfg_attr(feature = "ffi", derive(uniffi::Enum))]
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

/// The epoch a saved commit leaves, from saving it until its new epoch is
/// settled (see [`State::record_epoch`]).
const UNSETTLED_COMMIT_META: &str = "unsettled_commit";

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

    /// Saves a commit just before MLS applies it, and marks its epoch as not
    /// yet settled, so a crash after the merge still finds its lists.
    pub fn save_commit(&self, from_epoch: u64, signed: &[u8]) -> Result<()> {
        self.save_commit_with(from_epoch, signed, |_| Ok(()))
    }

    /// Saves a commit together with `also`, in one transaction.
    pub fn save_commit_with(
        &self,
        from_epoch: u64,
        signed: &[u8],
        also: impl FnOnce(&Store) -> Result<()>,
    ) -> Result<()> {
        self.store.atomically(|store| {
            store.put_commit_record(from_epoch, signed)?;
            store.set_meta(UNSETTLED_COMMIT_META, &from_epoch.to_le_bytes())?;
            also(store)
        })
    }

    /// Whether MLS applied a saved commit whose epoch isn't settled yet:
    /// until it is, nothing is followed or written.
    pub fn has_unsettled_epoch(&self) -> Result<bool> {
        let Some(from_epoch) = self.unsettled_commit()? else {
            return Ok(false);
        };
        Ok(!self.mls.is_active() || self.mls.epoch()? > from_epoch)
    }

    /// Settles the current epoch, once, after any epoch change: what the
    /// commit froze, which of its devices count, the epoch's key and its
    /// members, in one transaction, so a device's person is known before
    /// any op it writes under that key applies. It runs at start and before
    /// every commit is followed, also when this device was removed by the
    /// commit. The views' share of the freezing is left for
    /// [`Store::finish_view_work`].
    pub fn record_epoch(&self, config: &WorkspaceConfig) -> Result<()> {
        let Some(from_epoch) = self.unsettled_commit()? else {
            return self.record_members();
        };
        if !self.has_unsettled_epoch()? {
            // MLS refused the commit, or we stopped before it applied.
            return self.store.delete_meta(UNSETTLED_COMMIT_META);
        }
        let signed = self
            .store
            .commit_record(from_epoch)?
            .context("the commit being settled was not saved")?;
        let verified = commit::verify(&signed, from_epoch, &config.admin)?;
        let to_epoch = from_epoch.checked_add(1).context("no epoch after the last one")?;
        let binds_map = matches!(verified.wrapper, Wrapper::V3 { .. });
        let from_epoch_key = self
            .store
            .epoch_key(from_epoch)?
            .context("the key of the epoch the commit left isn't stored")?;
        let certificates = verified.certificates(from_epoch, &from_epoch_key)?;
        self.store.atomically(|store| {
            let mut frozen = Vec::new();
            for author in &verified.frozen {
                let was_dropped =
                    store.freeze_log(&author.member, author.last_seq, binds_map.then_some(to_epoch))?;
                frozen.push((author.member, was_dropped));
            }
            store.add_view_work(&frozen)?;
            if !Mls::commit_binding(&verified.commit)?.is_empty() {
                store.set_meta(BOUND_COMMIT_FOLLOWED_META, &[1])?;
            }
            if self.mls.is_active() {
                let members = self.mls.members()?;
                let counted =
                    store.count_certificates(&config.workspace_id, from_epoch, &members, &certificates)?;
                for record in counted {
                    store.put_device(&record, DeviceSource::Commit, now_ms()?)?;
                }
            }
            self.record_members()?;
            store.delete_meta(UNSETTLED_COMMIT_META)
        })
    }

    /// The device map's digest for a commit leaving `from_epoch` that binds
    /// it: the map before the commit, and the freezes bound so far with the
    /// commit's own, as recording them will leave them.
    pub fn map_key(&self, workspace_id: &str, from_epoch: u64, frozen: &[FrozenAuthor]) -> Result<[u8; 32]> {
        let to_epoch = from_epoch.checked_add(1).context("no epoch after the last one")?;
        let records = self.store.device_records()?;
        let mut freezes = self.store.bound_freezes()?;
        let already_frozen = self.store.frozen_authors()?;
        for author in frozen {
            if !already_frozen.iter().any(|(member, _)| *member == author.member)
                && !freezes.iter().any(|freeze| freeze.member == author.member)
            {
                freezes.push(BoundFreeze {
                    member: author.member,
                    last_seq: author.last_seq.min(MAX_STORED_NUMBER),
                    epoch: to_epoch,
                });
            }
        }
        map_digest(workspace_id, &records, &freezes)
    }

    fn record_members(&self) -> Result<()> {
        if self.mls.is_active() {
            let members = self.mls.members()?;
            let (epoch, key) = (self.mls.epoch()?, self.mls.epoch_key()?);
            // Storing never replaces a key, so one already there must be the
            // group's: anything else is a gap that must fail loudly.
            if let Some(stored) = self.store.epoch_key(epoch)? {
                ensure!(
                    stored == key,
                    "this device's key for the current epoch doesn't match its group"
                );
            }
            self.store.put_epoch(epoch, &key, &members)?;
            self.store.set_current_members(&members)?;
        }
        Ok(())
    }

    fn unsettled_commit(&self) -> Result<Option<u64>> {
        self.store
            .meta(UNSETTLED_COMMIT_META)?
            .map(|bytes| {
                Ok(u64::from_le_bytes(
                    bytes
                        .as_slice()
                        .try_into()
                        .context("damaged unsettled commit mark")?,
                ))
            })
            .transpose()
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
    /// The person this device belongs to: itself, unless it was linked to
    /// someone before it first started.
    pub person: PublicKey,
    pub mode: WorkspaceMode,
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
    /// Hashes of join requests the admin refused, so each is logged once.
    pub refused_requests: Mutex<std::collections::HashSet<[u8; 32]>>,
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
    /// Makes the next commit upload fail as an unreachable bucket would.
    #[cfg(test)]
    pub fail_next_commit_upload: std::sync::atomic::AtomicBool,
    /// Changes the next grant for a workspace with devices, as a dishonest
    /// admin's app might, after the digest was committed.
    #[cfg(test)]
    pub tamper_next_grant: Mutex<Option<crate::membership::admin::GrantTamper>>,
    /// Refuses the sealed op in the next batch written together, as a
    /// refused change would, after the batch's other ops were made.
    #[cfg(test)]
    pub refuse_next_sealed_write: std::sync::atomic::AtomicBool,
    /// Held while a huddle's audience is worked out and published, so an
    /// older result can never replace a newer one.
    pub audience_lock: Mutex<()>,
    /// Held while the admin's app acts on device changes, so two passes
    /// can't act on the same change.
    pub device_actions_lock: tokio::sync::Mutex<()>,
    /// One pass at a time over the links this device confirmed.
    pub pending_links_lock: tokio::sync::Mutex<()>,
    /// When each owed link welcome last went up, by its code and device.
    pub welcomes_sent:
        Mutex<std::collections::HashMap<(crate::membership::link::Token, PublicKey), tokio::time::Instant>>,
    /// The link code this device shows, with its secrets, in memory only.
    pub(crate) open_link: Mutex<Option<linking::OpenLink>>,
    /// Pauses the next audience refresh after it read who may hear the
    /// call: it says so on the first channel, then waits on the second.
    #[cfg(test)]
    pub audience_pause: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    /// Pauses the next removal after it collected the removed devices'
    /// logs, before it reads where to freeze them.
    #[cfg(test)]
    pub removal_pause: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
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

    /// Whether this device may make `op` in `epoch` (the newest one it
    /// holds, for what it may do now), by the rule every member enforces.
    pub(crate) fn may(&self, store: &crate::storage::Store, epoch: u64, op: &crate::ops::Op) -> Result<bool> {
        store.is_permitted(op, &self.me(), epoch, &self.inner.config.admin)
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

    /// The person this device belongs to, as a member id (hex). What this
    /// device signs with is [`Workspace::me`].
    pub fn my_id(&self) -> String {
        member_id(&self.inner.person)
    }

    /// This device's own member id (hex), as `remove_devices` takes it.
    pub fn my_device_id(&self) -> String {
        member_id(&self.me())
    }

    /// The person this device belongs to: itself, unless it was linked to
    /// someone.
    pub fn my_person(&self) -> PublicKey {
        self.inner.person
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
