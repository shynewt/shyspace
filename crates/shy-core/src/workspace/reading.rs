//! A workspace a new device only reads, with what its person's device
//! handed it, while it waits for the admin's app to add it. It lives in its
//! own folder next to the real one and is built only over a reading bucket,
//! which refuses every write, so nothing it does can reach the bucket. It
//! has no MLS group and no endpoint; its sync pass only pulls and applies.

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;

use super::{DataDir, Networking, Workspace};
use crate::{
    config::{self, WorkspaceConfig},
    membership::{handover::Handover, link::LinkCode},
    storage::{ApplyContext, Store},
    transport::bucket::{Connector, read_only::ReadingConnector},
};

/// What a write says on a device that only reads.
pub(crate) const READING_ONLY: &str = "This device can read now. It can send once your workspace adds it.";

const LINKING_PERSON_META: &str = "linking_person";

/// Whether a workspace is a member's, or a new device's that only reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceMode {
    Member,
    Linking,
}

/// What a workspace starts with: a member's bucket, or a reading one, so a
/// workspace in [`WorkspaceMode::Linking`] can't get a writable bucket.
pub(super) enum Opening {
    Member(Connector),
    Linking(ReadingConnector),
}

impl Opening {
    pub(super) fn into_parts(self) -> (Connector, WorkspaceMode) {
        match self {
            Self::Member(connect) => (connect, WorkspaceMode::Member),
            Self::Linking(connect) => (connect.into_connector(), WorkspaceMode::Linking),
        }
    }
}

/// The person a linking device asked to be linked to.
pub(super) fn linking_person(store: &Store) -> Result<PublicKey> {
    let bytes = store
        .meta(LINKING_PERSON_META)?
        .context("a reading workspace without the person it links to")?;
    Ok(PublicKey::from_bytes(bytes.as_slice().try_into().context(
        "the person a reading workspace links to is corrupt",
    )?)?)
}

impl Workspace {
    /// Fills the linking store in `data` with what `code`'s voucher handed
    /// this device, forgets the one-time secret, then opens it for reading.
    pub(crate) async fn open_reading_from_handover(
        data: &DataDir,
        code: &LinkCode,
        handover: Handover,
        connect: ReadingConnector,
    ) -> Result<Self> {
        ensure!(
            handover
                .history
                .epochs
                .iter()
                .all(|entry| entry.epoch <= handover.epoch),
            "a hand-over with keys newer than its voucher's"
        );
        ensure!(
            !Self::exists(&data.path),
            "this device already opened a hand-over"
        );
        let local = data.open_local().await?;
        let identity = &local.identity;
        let config = WorkspaceConfig {
            workspace_id: code.workspace_id.clone(),
            name: code.workspace_name.clone(),
            admin: code.admin,
            bucket: handover.bucket.clone(),
        };
        let context = ApplyContext {
            workspace_id: &config.workspace_id,
            admin: config.admin,
            me: identity.public(),
        };
        let store = &local.store;
        handover
            .history
            .store_with_devices(&handover.devices, &handover.bound_frozen, store, context)?;
        config.save_in_store(store)?;
        config::update_bucket(store, &config.bucket, handover.bucket_lamport)?;
        config::record_settings_op(store, config::NO_SETTINGS_OP)?;
        store.set_meta(LINKING_PERSON_META, code.person.as_bytes())?;
        store.delete_meta(super::linking_store::HANDOVER_SECRET_META)?;
        config.mark_complete(&data.path)?;
        drop(local);
        Self::open_reading(data, connect).await
    }

    /// Opens a reading store made earlier.
    pub(crate) async fn open_reading(data: &DataDir, connect: ReadingConnector) -> Result<Self> {
        let local = data.open_local().await?;
        let config = WorkspaceConfig::load(&data.path, &local.store)?;
        let mls = local.open_mls(data, None)?;
        Self::start(
            data,
            local,
            mls,
            config,
            Opening::Linking(connect),
            Networking::Offline,
        )
        .await
    }

    pub(crate) fn is_reading_only(&self) -> bool {
        self.inner.mode == WorkspaceMode::Linking
    }
}
