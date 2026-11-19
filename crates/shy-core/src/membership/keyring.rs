//! The history a newcomer receives, so they can read everything written
//! before they joined and know which removed members' logs have ended.

use anyhow::Result;
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use super::commit::FrozenAuthor;
use crate::{crypto::EpochKey, storage::Store, workspace::State};

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
    fn collect(store: &Store) -> Result<Self> {
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

    pub fn store_into(self, store: &Store) -> Result<()> {
        for entry in self.epochs {
            store.put_epoch(entry.epoch, &entry.key, &entry.members)?;
        }
        for frozen in self.frozen {
            store.freeze_author(&frozen.member, frozen.last_seq)?;
        }
        Ok(())
    }
}

/// Encrypts the whole history for the members of the current epoch. Returns
/// the epoch it was sealed in, which is where the newcomer looks for it.
pub(crate) fn seal(state: &mut State) -> Result<(u64, Vec<u8>)> {
    let history = KeyHistory::collect(&state.store)?;
    let epoch = state.mls.epoch()?;
    Ok((
        epoch,
        state.mls.seal_for_members(&postcard::to_stdvec(&history)?)?,
    ))
}
