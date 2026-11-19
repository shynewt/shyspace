//! The key history a newcomer receives, so they can read everything written
//! before they joined.

use anyhow::Result;
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use crate::{crypto::EpochKey, workspace::State};

/// One past epoch: its key and who could write in it.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct KeyringEntry {
    pub epoch: u64,
    pub key: EpochKey,
    pub members: Vec<PublicKey>,
}

/// Encrypts the whole history for the members of the current epoch. Returns
/// the epoch it was sealed in, which is where the newcomer looks for it.
pub(crate) fn seal(state: &mut State) -> Result<(u64, Vec<u8>)> {
    let mut keyring = Vec::new();
    for (epoch, key) in state.store.epoch_keys()? {
        keyring.push(KeyringEntry {
            epoch,
            key,
            members: state.store.epoch_members(epoch)?,
        });
    }
    let epoch = state.mls.epoch()?;
    Ok((
        epoch,
        state.mls.seal_for_members(&postcard::to_stdvec(&keyring)?)?,
    ))
}
