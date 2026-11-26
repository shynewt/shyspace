//! Who may make which change. Admin-only changes, and deleting anyone's
//! content, take the admin's own device; the admin's other devices get no
//! such power. Editing, renaming and deleting your own content take any
//! device of the person who created it, judged at the epoch the op was
//! sealed in, so a device can't reach back to before it was linked.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::ops::{OWNER_PREFIX_BYTES, Op, owner_prefix_of};

impl Store {
    /// Whether `author` may make `op`, sealed in `epoch`, in a workspace
    /// run by `admin`.
    pub fn is_permitted(&self, op: &Op, author: &PublicKey, epoch: u64, admin: &PublicKey) -> Result<bool> {
        Ok(match op {
            Op::StorageSettings { .. } | Op::WorkspaceIcon { .. } | Op::WorkspaceLook { .. } => {
                author == admin
            }
            Op::MessageEdit { id, .. } | Op::FileRename { id, .. } => self.is_owner(id, author, epoch)?,
            Op::Delete { id } => author == admin || self.is_owner(id, author, epoch)?,
            _ => true,
        })
    }

    /// Whether `author`'s person at `epoch` created `id`: the id carries the
    /// start of its creating device's key, and that device's person is the
    /// one its row gave it by then, or the device itself.
    fn is_owner(&self, id: &str, author: &PublicKey, epoch: u64) -> Result<bool> {
        let Some(prefix) = owner_prefix_of(id) else {
            return Ok(false);
        };
        let person = self.person_at(author, epoch)?;
        // A scan of the devices table by key prefix; it holds a few rows per
        // person at most.
        let linked_owner: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT person FROM devices WHERE substr(device, 1, ?) = ? AND added_epoch <= ?
                 ORDER BY added_epoch, device LIMIT 1",
                params![OWNER_PREFIX_BYTES, prefix.as_slice(), epoch],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match linked_owner {
            Some(owner) => owner == person.as_bytes(),
            None => person.as_bytes()[..OWNER_PREFIX_BYTES] == prefix,
        })
    }

    /// The newest epoch this device holds a key for.
    pub fn newest_epoch(&self) -> Result<u64> {
        let newest: Option<u64> = self
            .db
            .query_row("SELECT MAX(epoch) FROM epoch_keys", [], |row| row.get(0))?;
        Ok(newest.unwrap_or_default())
    }
}
