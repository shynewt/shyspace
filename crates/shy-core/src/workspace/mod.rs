//! The public entry point: one workspace, stored in one data directory.

mod actions;
mod lifecycle;
mod reads;

pub use lifecycle::DataDir;
pub(crate) use lifecycle::{MLS_FILE, STORE_FILE};

use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use iroh::{PublicKey, SecretKey};
use tokio::sync::{Notify, broadcast};

use crate::{
    config::WorkspaceConfig,
    keys::DeviceKey,
    membership::{admin::drop_unsealed_deliveries, mls::Mls},
    storage::Store,
    sync::SyncStatus,
    transport::{bucket::Bucket, gossip::Net},
    views::{Change, member_id},
};

/// Whether to go online. Tests run several members against one in-memory
/// bucket without networking.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Networking {
    Online,
    Offline,
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
    pub bucket: Bucket,
    pub net: Option<Net>,
    pub events: broadcast::Sender<Change>,
    /// Wakes the sync loop early: after a local write or a peer's nudge.
    pub wake: Notify,
    pub sync_lock: tokio::sync::Mutex<()>,
    pub recovery_lock: tokio::sync::Mutex<()>,
    pub status: Mutex<SyncStatus>,
}

#[derive(Clone)]
pub struct Workspace {
    pub(crate) inner: Arc<Inner>,
}

impl Workspace {
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

    pub async fn shutdown(&self) {
        if let Some(net) = &self.inner.net {
            net.shutdown().await;
        }
    }
}
