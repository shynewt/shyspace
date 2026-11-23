//! Changing the bucket key after a member is removed: they keep the old key
//! until the admin deletes it at the provider. A removal starts a rotation
//! on the admin's device; the admin saves a new key (sent to current
//! members only, under the epoch key the removal replaced), sees who has
//! moved to it, deletes the old key at the provider, and the app proves it
//! dead. The old secrets are kept, in the encrypted store on the admin's
//! device only, just until each is proved dead or the rotation is stopped.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::Workspace;
use crate::{
    storage::Store,
    transport::{bucket::BucketConfig, heads::key_tag},
    views::{Change, MemberMove, OldKeyCheck, OldKeyState, StorageProvider, StorageRotation, member_id},
};

const ROTATION_META: &str = "storage_rotation";

#[derive(Serialize, Deserialize)]
struct Rotation {
    /// Keys replaced since the rotation began, until each is proved dead.
    old_keys: Vec<OldKey>,
    old_key: OldKeyState,
}

#[derive(Clone, Serialize, Deserialize)]
struct OldKey {
    access_key_id: String,
    secret_access_key: String,
}

fn load(store: &Store) -> Result<Option<Rotation>> {
    store
        .meta(ROTATION_META)?
        .map(|bytes| serde_json::from_slice(&bytes))
        .transpose()
        .map_err(|error| anyhow::anyhow!("unreadable bucket key rotation: {error}"))
}

fn save(store: &Store, rotation: &Rotation) -> Result<()> {
    store.set_meta(ROTATION_META, &serde_json::to_vec(rotation)?)
}

fn provider_of(endpoint: &str) -> StorageProvider {
    let host = endpoint.to_ascii_lowercase();
    if host.contains("scw.cloud") {
        StorageProvider::Scaleway
    } else if host.contains("r2.cloudflarestorage.com") {
        StorageProvider::R2
    } else if host.contains("backblazeb2.com") {
        StorageProvider::B2
    } else if host.contains("amazonaws.com") {
        StorageProvider::S3
    } else {
        StorageProvider::Other
    }
}

/// Whether the bucket refused these keys (401 or 403), rather than
/// failing for any other reason.
fn is_refused(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<object_store::Error>(),
            Some(object_store::Error::PermissionDenied { .. } | object_store::Error::Unauthenticated { .. })
        )
    })
}

impl Workspace {
    /// Starts a rotation, if none is under way: a member was removed.
    pub(crate) fn start_storage_rotation(store: &Store) -> Result<()> {
        if load(store)?.is_none() {
            save(
                store,
                &Rotation {
                    old_keys: Vec::new(),
                    old_key: OldKeyState::NotChecked,
                },
            )?;
        }
        Ok(())
    }

    /// Keeps `current` to prove it dead later, as `next` replaces it.
    pub(crate) fn keep_replaced_key(
        store: &Store,
        current: &BucketConfig,
        next: &BucketConfig,
    ) -> Result<()> {
        let is_replaced = current.access_key_id != next.access_key_id
            || current.secret_access_key != next.secret_access_key;
        if !is_replaced {
            return Ok(());
        }
        let mut rotation = load(store)?.unwrap_or(Rotation {
            old_keys: Vec::new(),
            old_key: OldKeyState::NotChecked,
        });
        rotation
            .old_keys
            .retain(|old| old.access_key_id != next.access_key_id);
        rotation.old_keys.push(OldKey {
            access_key_id: current.access_key_id.clone(),
            secret_access_key: current.secret_access_key.clone(),
        });
        rotation.old_key = OldKeyState::NotChecked;
        save(store, &rotation)
    }

    /// The rotation under way, for the admin; `None` when nothing is due.
    /// Reads each member's head, so it costs a request per member.
    pub async fn storage_rotation(&self) -> Result<Option<StorageRotation>> {
        if !self.is_admin() {
            return Ok(None);
        }
        let Some(rotation) = load(&self.state().store)? else {
            return Ok(None);
        };
        let config = self.bucket_config()?;
        let has_new_key = !rotation.old_keys.is_empty();
        let current_key = key_tag(&config.access_key_id);
        let me = self.me();
        let mut members = Vec::new();
        for member in self.current_members()? {
            if member == me {
                continue;
            }
            let head = self.bucket().head(&member).await?;
            members.push(MemberMove {
                member: member_id(&member),
                has_moved: has_new_key && head.is_some_and(|head| head.key == current_key),
            });
        }
        Ok(Some(StorageRotation {
            provider: provider_of(&config.endpoint),
            has_new_key,
            members,
            old_key: rotation.old_key,
        }))
    }

    /// Proves the replaced keys deleted at the provider: each is tried with
    /// one listing, right after the current key does the same, so a refusal
    /// can't be a network, clock or endpoint problem. Only a refusal counts
    /// as deleted; that key's secret is erased at once, and once none is
    /// left the rotation ends.
    pub async fn check_old_key_deleted(&self) -> Result<OldKeyCheck> {
        self.ensure_admin()?;
        let rotation = load(&self.state().store)?;
        let old_keys = rotation.map(|rotation| rotation.old_keys).unwrap_or_default();
        ensure!(!old_keys.is_empty(), "save the new bucket key first");
        if self.bucket().probe_access().await.is_err() {
            return self.note_old_key_check(&[], OldKeyCheck::Unknown);
        }
        let config = self.bucket_config()?;
        let mut dead = Vec::new();
        let mut check = OldKeyCheck::Dead;
        for old in &old_keys {
            let old_config = BucketConfig {
                access_key_id: old.access_key_id.clone(),
                secret_access_key: old.secret_access_key.clone(),
                ..config.clone()
            };
            let bucket = (self.inner.connect)(&self.inner.config.workspace_id, &old_config)?;
            match bucket.probe_access().await {
                Ok(()) => check = OldKeyCheck::StillWorks,
                Err(error) if is_refused(&error) => dead.push(old.access_key_id.clone()),
                Err(_) if check == OldKeyCheck::Dead => check = OldKeyCheck::Unknown,
                Err(_) => {}
            }
        }
        self.note_old_key_check(&dead, check)
    }

    /// Erases the keys found `dead`, and ends the rotation once none is left.
    fn note_old_key_check(&self, dead: &[String], check: OldKeyCheck) -> Result<OldKeyCheck> {
        {
            let state = self.state();
            let Some(mut rotation) = load(&state.store)? else {
                return Ok(check);
            };
            rotation.old_keys.retain(|old| !dead.contains(&old.access_key_id));
            if rotation.old_keys.is_empty() {
                state.store.delete_meta(ROTATION_META)?;
            } else {
                rotation.old_key = match check {
                    OldKeyCheck::StillWorks => OldKeyState::StillWorks,
                    OldKeyCheck::Dead | OldKeyCheck::Unknown => OldKeyState::Unknown,
                };
                save(&state.store, &rotation)?;
            }
        }
        self.emit(Change::Storage);
        Ok(check)
    }

    /// Ends the rotation without proving the old keys dead, erasing them.
    pub fn cancel_storage_rotation(&self) -> Result<()> {
        self.ensure_admin()?;
        self.state().store.delete_meta(ROTATION_META)?;
        self.emit(Change::Storage);
        Ok(())
    }

    /// Checks again a check that couldn't tell; part of the 10-minute duties.
    pub(crate) async fn recheck_old_key_if_unknown(&self) -> Result<()> {
        let rotation = load(&self.state().store)?;
        if rotation.is_some_and(|rotation| rotation.old_key == OldKeyState::Unknown) {
            self.check_old_key_deleted().await?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn kept_old_keys(&self) -> Result<usize> {
        Ok(load(&self.state().store)?.map_or(0, |rotation| rotation.old_keys.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_is_told_from_the_endpoint() {
        let cases = [
            ("https://s3.fr-par.scw.cloud", StorageProvider::Scaleway),
            ("https://abc.r2.cloudflarestorage.com", StorageProvider::R2),
            ("https://s3.eu-central-003.backblazeb2.com", StorageProvider::B2),
            ("https://s3.eu-west-1.amazonaws.com", StorageProvider::S3),
            ("http://localhost:9000", StorageProvider::Other),
        ];
        for (endpoint, provider) in cases {
            assert_eq!(provider_of(endpoint), provider, "{endpoint}");
        }
    }
}
