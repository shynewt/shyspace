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
    config,
    ops::Op,
    storage::Store,
    transport::{
        bucket::BucketConfig,
        heads::{is_refused, key_tag},
    },
    views::{Change, MemberMove, OldKeyCheck, OldKeyState, StorageProvider, StorageRotation, member_id},
};

const ROTATION_META: &str = "storage_rotation";

#[derive(Default, Serialize, Deserialize)]
struct Rotation {
    /// Access key ids a removed member had: never to be used again.
    compromised: Vec<String>,
    /// Keys replaced since the rotation began, until each is proved dead.
    old_keys: Vec<OldKey>,
    old_key: OldKeyState,
}

impl Rotation {
    /// Over once every replaced key is dead and the key in use is one no
    /// removed member had.
    fn is_over(&self, current_key_id: &str) -> bool {
        self.old_keys.is_empty() && !self.compromised.iter().any(|id| id == current_key_id)
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct OldKey {
    access_key_id: String,
    secret_access_key: String,
}

fn load(store: &Store) -> Result<Option<Rotation>> {
    let Some(bytes) = store.meta(ROTATION_META)? else {
        return Ok(None);
    };
    // The error is left out: it could quote a secret.
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| anyhow::anyhow!("the saved bucket key rotation is unreadable"))
}

/// Saves `rotation`, or ends it if it is over. The recovery snapshot holds
/// this too (the old secrets, encrypted), so it is refreshed either way.
fn save(store: &Store, rotation: &Rotation, current_key_id: &str) -> Result<()> {
    match rotation.is_over(current_key_id) {
        true => store.delete_meta(ROTATION_META)?,
        false => store.set_meta(ROTATION_META, &serde_json::to_vec(rotation)?)?,
    }
    Workspace::mark_recovery_snapshot_stale(store)
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

impl Workspace {
    /// A member was removed: the key in use is now one they have, so a
    /// rotation is due until a new key is in use and the old ones are dead.
    pub(crate) fn start_storage_rotation(store: &Store) -> Result<()> {
        let current = config::current_bucket(store)?;
        let mut rotation = load(store)?.unwrap_or_default();
        if !rotation.compromised.contains(&current.access_key_id) {
            rotation.compromised.push(current.access_key_id.clone());
        }
        rotation.old_key = OldKeyState::NotChecked;
        save(store, &rotation, &current.access_key_id)
    }

    /// Refuses a key a removed member had.
    fn ensure_not_compromised(&self, next: &BucketConfig) -> Result<()> {
        let rotation = load(&self.state().store)?;
        let is_compromised =
            rotation.is_some_and(|rotation| rotation.compromised.contains(&next.access_key_id));
        ensure!(
            !is_compromised,
            "a removed member had this key; make a new one at your storage provider"
        );
        Ok(())
    }

    /// Sends `next` to every current member as the bucket settings, keeping
    /// the key it replaces (read in the same transaction) to prove it dead
    /// later: never one without the other, so the key in use is never
    /// offered for deletion.
    pub(crate) fn replace_bucket_key(&self, next: BucketConfig) -> Result<()> {
        self.ensure_not_compromised(&next)?;
        self.write_with(|store, _| {
            let current = config::current_bucket(store)?;
            let is_replaced = current.access_key_id != next.access_key_id
                || current.secret_access_key != next.secret_access_key;
            if is_replaced {
                let mut rotation = load(store)?.unwrap_or_default();
                rotation
                    .old_keys
                    .retain(|old| old.access_key_id != next.access_key_id);
                rotation.old_keys.push(OldKey {
                    access_key_id: current.access_key_id,
                    secret_access_key: current.secret_access_key,
                });
                rotation.old_key = OldKeyState::NotChecked;
                save(store, &rotation, &next.access_key_id)?;
            }
            Ok(Op::StorageSettings { bucket: next })
        })?;
        self.emit(Change::KeyRotation);
        Ok(())
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
        let has_new_key = !rotation.compromised.contains(&config.access_key_id);
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
    /// one read, right after the current key does the same, so a refusal
    /// can't be a network, clock or endpoint problem. Only a refusal counts
    /// as deleted; that key's secret is erased at once, and the rotation
    /// ends once none is left and the key in use is one no removed member had.
    pub async fn check_old_key_deleted(&self) -> Result<OldKeyCheck> {
        self.ensure_admin()?;
        let old_keys = load(&self.state().store)?
            .map(|rotation| rotation.old_keys)
            .unwrap_or_default();
        ensure!(!old_keys.is_empty(), "save the new bucket key first");
        let me = self.me();
        if self.bucket().probe_access(&me).await.is_err() {
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
            match bucket.probe_access(&me).await {
                Ok(()) => check = OldKeyCheck::StillWorks,
                Err(error) if is_refused(&error) => dead.push(old.access_key_id.clone()),
                Err(_) if check == OldKeyCheck::Dead => check = OldKeyCheck::Unknown,
                Err(_) => {}
            }
        }
        self.note_old_key_check(&dead, check)
    }

    /// Erases the keys found `dead`, and ends the rotation if it is over.
    fn note_old_key_check(&self, dead: &[String], check: OldKeyCheck) -> Result<OldKeyCheck> {
        {
            let state = self.state();
            let Some(mut rotation) = load(&state.store)? else {
                return Ok(check);
            };
            let current = config::current_bucket(&state.store)?;
            rotation.old_keys.retain(|old| !dead.contains(&old.access_key_id));
            rotation.old_key = match check {
                OldKeyCheck::StillWorks => OldKeyState::StillWorks,
                OldKeyCheck::Dead if rotation.old_keys.is_empty() => OldKeyState::NotChecked,
                OldKeyCheck::Dead | OldKeyCheck::Unknown => OldKeyState::Unknown,
            };
            save(&state.store, &rotation, &current.access_key_id)?;
        }
        self.emit(Change::KeyRotation);
        Ok(check)
    }

    /// Ends the rotation without proving the old keys dead, erasing them.
    pub fn cancel_storage_rotation(&self) -> Result<()> {
        self.ensure_admin()?;
        {
            let state = self.state();
            state.store.delete_meta(ROTATION_META)?;
            Workspace::mark_recovery_snapshot_stale(&state.store)?;
        }
        self.emit(Change::KeyRotation);
        Ok(())
    }

    /// Checks again a check that couldn't tell; part of the 10-minute duties.
    pub(crate) async fn recheck_old_key_if_unknown(&self) -> Result<()> {
        let rotation = load(&self.state().store)?;
        let is_unknown = rotation.is_some_and(|rotation| {
            rotation.old_key == OldKeyState::Unknown && !rotation.old_keys.is_empty()
        });
        if is_unknown {
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

    /// A real S3 endpoint answering every request as a provider does for a
    /// deleted key: 403 InvalidAccessKeyId.
    async fn refusing_endpoint() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 8192];
                let _ = socket.read(&mut request).await;
                let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>InvalidAccessKeyId</Code><Message>The access key does not exist.</Message></Error>";
                let response = format!(
                    "HTTP/1.1 403 Forbidden\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn a_real_s3_bucket_refusing_a_deleted_key_counts_as_refused() {
        let config = BucketConfig {
            endpoint: refusing_endpoint().await,
            region: "us-east-1".into(),
            bucket: "shyspace".into(),
            access_key_id: "deleted-key".into(),
            secret_access_key: "deleted-secret".into(),
        };
        let error = config
            .connect("workspace-id")
            .unwrap()
            .probe_access(&iroh::SecretKey::generate().public())
            .await
            .expect_err("the provider refuses the key");
        assert!(is_refused(&error), "a 403 must count as refused: {error:#}");
    }
}
