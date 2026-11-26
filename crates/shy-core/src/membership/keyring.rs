//! The history a newcomer receives, so they can read everything written
//! before they joined and know which removed members' logs have ended.

use anyhow::Result;
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use super::{
    commit::FrozenAuthor,
    device::{BoundFreeze, DeviceRecord},
};
use crate::{
    crypto::EpochKey,
    storage::{ApplyContext, DeviceSource, Store},
};

/// One past epoch: its key and who could write in it.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct KeyringEntry {
    pub epoch: u64,
    pub key: EpochKey,
    pub members: Vec<PublicKey>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct KeyHistory {
    pub epochs: Vec<KeyringEntry>,
    pub frozen: Vec<FrozenAuthor>,
}

impl KeyHistory {
    pub fn collect(store: &Store) -> Result<Self> {
        let mut epochs = Vec::new();
        for (epoch, key) in store.epoch_keys()? {
            epochs.push(KeyringEntry {
                epoch,
                key,
                members: store.epoch_members(epoch)?,
            });
        }
        let frozen = store
            .frozen_authors()?
            .into_iter()
            .map(|(member, last_seq)| FrozenAuthor { member, last_seq })
            .collect();
        Ok(Self { epochs, frozen })
    }

    /// Stores the history with the device map a grant brought, as given:
    /// the digest the newcomer checked proves the device records and bound
    /// freezes are the ones every member counts. The epochs, their members
    /// and older freezes are on the admin's word, as before devices.
    pub fn store_with_devices(
        self,
        devices: &[DeviceRecord],
        bound_frozen: &[BoundFreeze],
        store: &Store,
        context: ApplyContext<'_>,
    ) -> Result<()> {
        store.atomically(|store| {
            for freeze in bound_frozen {
                store.freeze_log(&freeze.member, freeze.last_seq, Some(freeze.epoch))?;
            }
            for record in devices {
                store.put_device(record, DeviceSource::Grant, 0)?;
            }
            self.store_into(store, context)
        })
    }

    pub fn store_into(self, store: &Store, context: ApplyContext<'_>) -> Result<()> {
        for entry in self.epochs {
            store.put_epoch(entry.epoch, &entry.key, &entry.members)?;
        }
        for frozen in self.frozen {
            store.freeze_author(&frozen.member, frozen.last_seq, context)?;
        }
        Ok(())
    }
}
