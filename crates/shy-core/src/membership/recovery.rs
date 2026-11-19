//! Admin recovery. If the admin's device is lost, membership could never change
//! again. So the admin keeps a recovery kit (a code to store in a password
//! manager), and after every membership change their app uploads an encrypted
//! snapshot of its admin state to the bucket. Restoring on a new device needs
//! only the kit; messages and pages come back from the bucket as usual.
//!
//! Only restore when the old device is truly gone: two live copies of the
//! admin would make conflicting membership changes.

use anyhow::{Context, Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::commit;
use crate::{
    config::{CONFIG_FILE, write_private_file},
    keys::DeviceKey,
    storage::{snapshot_database, snapshot_store_without_content},
    transport::bucket::{Bucket, BucketConfig},
    workspace::{DataDir, MLS_FILE, Networking, STORE_FILE, Workspace},
};

const KIT_PREFIX: &str = "shyrec1_";
const RECOVERY_KEY_META: &str = "recovery_key";
/// Bumped on every membership change; the snapshot is current when the
/// uploaded generation matches.
const CHANGE_GENERATION_META: &str = "recovery_change_generation";
const UPLOADED_GENERATION_META: &str = "recovery_uploaded_generation";
const SNAPSHOT_LABEL: &str = "shyspace recovery snapshot v1";

/// Everything needed to restore the admin on a new device. A secret: it
/// unlocks the admin role and contains the bucket credentials.
#[derive(Serialize, Deserialize)]
struct RecoveryKit {
    workspace_id: String,
    admin: PublicKey,
    bucket: BucketConfig,
    recovery_key: [u8; 32],
}

impl RecoveryKit {
    fn encode(&self) -> Result<Zeroizing<String>> {
        let bytes = Zeroizing::new(postcard::to_stdvec(self)?);
        Ok(Zeroizing::new(format!(
            "{KIT_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(bytes.as_slice())
        )))
    }

    fn decode(code: &str) -> Result<Self> {
        let payload = code
            .trim()
            .strip_prefix(KIT_PREFIX)
            .context("not a shyspace recovery kit")?;
        let bytes = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(payload)
                .context("the recovery kit is damaged")?,
        );
        postcard::from_bytes(&bytes).context("the recovery kit is damaged")
    }
}

/// The admin's local state, minus what the bucket can give back.
#[derive(Serialize, Deserialize)]
struct Snapshot {
    /// The MLS epoch this snapshot is at. Restore refuses it if the bucket
    /// holds an admin commit from this epoch onward, i.e. it's stale.
    epoch: u64,
    device_key: [u8; 32],
    workspace_json: Vec<u8>,
    store_db: Vec<u8>,
    mls_db: Vec<u8>,
}

fn snapshot_cipher(recovery_key: &[u8; 32]) -> XChaCha20Poly1305 {
    let key = Zeroizing::new(blake3::derive_key(SNAPSHOT_LABEL, recovery_key));
    XChaCha20Poly1305::new((&*key).into())
}

fn seal_snapshot(recovery_key: &[u8; 32], workspace_id: &str, snapshot: &Snapshot) -> Result<Vec<u8>> {
    let plaintext = Zeroizing::new(postcard::to_stdvec(snapshot)?);
    let nonce = XNonce::generate();
    let ciphertext = snapshot_cipher(recovery_key)
        .encrypt(
            &nonce,
            Payload {
                msg: &plaintext,
                aad: workspace_id.as_bytes(),
            },
        )
        .map_err(|_| anyhow!("encrypting the recovery snapshot failed"))?;
    Ok([nonce.as_slice(), &ciphertext].concat())
}

fn open_snapshot(recovery_key: &[u8; 32], workspace_id: &str, sealed: &[u8]) -> Result<Snapshot> {
    ensure!(sealed.len() > 24, "the recovery snapshot is damaged");
    let (nonce, ciphertext) = sealed.split_at(24);
    let nonce: [u8; 24] = nonce.try_into()?;
    let plaintext = Zeroizing::new(
        snapshot_cipher(recovery_key)
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: ciphertext,
                    aad: workspace_id.as_bytes(),
                },
            )
            .map_err(|_| anyhow!("the recovery snapshot doesn't match this kit"))?,
    );
    Ok(postcard::from_bytes(&plaintext)?)
}

fn generation(store: &crate::storage::Store, key: &str) -> Result<u64> {
    match store.meta(key)? {
        None => Ok(0),
        Some(bytes) => Ok(u64::from_be_bytes(
            bytes
                .try_into()
                .map_err(|_| anyhow!("stored recovery generation is corrupt"))?,
        )),
    }
}

impl Workspace {
    /// The admin's recovery kit, created the first time. Uploads a fresh
    /// snapshot so the kit works right away.
    pub async fn recovery_kit(&self) -> Result<Zeroizing<String>> {
        ensure!(self.is_admin(), "only the workspace admin has a recovery kit");
        let recovery_key = {
            let store = &self.state().store;
            match store.meta(RECOVERY_KEY_META)? {
                Some(bytes) => {
                    let bytes = Zeroizing::new(bytes);
                    bytes
                        .as_slice()
                        .try_into()
                        .map_err(|_| anyhow!("the stored recovery key is corrupt"))?
                }
                None => {
                    let key: [u8; 32] = rand::random();
                    store.set_meta(RECOVERY_KEY_META, &key)?;
                    key
                }
            }
        };
        let recovery_key = Zeroizing::new(recovery_key);
        self.state().store.delete_meta(UPLOADED_GENERATION_META)?;
        self.refresh_recovery_snapshot().await?;
        let config = &self.inner.config;
        RecoveryKit {
            workspace_id: config.workspace_id.clone(),
            admin: config.admin,
            bucket: config.bucket.clone(),
            recovery_key: *recovery_key,
        }
        .encode()
    }

    /// Records that admin state changed and needs a new snapshot.
    pub(crate) fn mark_recovery_snapshot_stale(store: &crate::storage::Store) -> Result<()> {
        let next = generation(store, CHANGE_GENERATION_META)? + 1;
        store.set_meta(CHANGE_GENERATION_META, &next.to_be_bytes())
    }

    /// Uploads a new snapshot if membership changed since the last one. Does
    /// nothing until the admin has created a recovery kit.
    pub(crate) async fn refresh_recovery_snapshot(&self) -> Result<()> {
        // One refresh at a time, so an older snapshot can't land after a newer one.
        let _guard = self.inner.recovery_lock.lock().await;
        let (generation_taken, sealed) = {
            let state = self.state();
            let Some(recovery_key) = state.store.meta(RECOVERY_KEY_META)? else {
                return Ok(());
            };
            let current = generation(&state.store, CHANGE_GENERATION_META)?;
            let is_uploaded = state.store.meta(UPLOADED_GENERATION_META)?.is_some()
                && generation(&state.store, UPLOADED_GENERATION_META)? == current;
            if is_uploaded {
                return Ok(());
            }
            let recovery_key: Zeroizing<[u8; 32]> = Zeroizing::new(
                recovery_key
                    .as_slice()
                    .try_into()
                    .map_err(|_| anyhow!("the stored recovery key is corrupt"))?,
            );
            // Taken under the state lock so the store and MLS copies agree.
            let snapshot = self.take_snapshot(state.mls.epoch()?)?;
            let sealed = seal_snapshot(&recovery_key, &self.inner.config.workspace_id, &snapshot)?;
            (current, sealed)
        };
        self.inner.bucket.put_recovery_snapshot(sealed).await?;
        self.state()
            .store
            .set_meta(UPLOADED_GENERATION_META, &generation_taken.to_be_bytes())
    }

    fn take_snapshot(&self, epoch: u64) -> Result<Snapshot> {
        let data = &self.inner.data;
        let scratch = tempfile::Builder::new()
            .prefix("recovery-")
            .tempdir_in(&data.path)?;
        let device_key = &self.inner.device_key;
        let store_copy = scratch.path().join(STORE_FILE);
        let mls_copy = scratch.path().join(MLS_FILE);
        snapshot_store_without_content(
            &data.store_path(),
            &device_key.database_key(STORE_FILE),
            &store_copy,
        )?;
        snapshot_database(&data.mls_path(), &device_key.database_key(MLS_FILE), &mls_copy)?;
        Ok(Snapshot {
            epoch,
            device_key: *device_key.to_bytes(),
            workspace_json: std::fs::read(data.path.join(CONFIG_FILE))?,
            store_db: std::fs::read(&store_copy)?,
            mls_db: std::fs::read(&mls_copy)?,
        })
    }

    /// Restores the admin from a recovery kit onto this device, then catches
    /// up from the bucket. Only for when the old admin device is gone.
    pub async fn restore(data: &DataDir, recovery_kit: &str, networking: Networking) -> Result<Self> {
        let kit = RecoveryKit::decode(recovery_kit)?;
        let bucket = kit.bucket.connect(&kit.workspace_id)?;
        Self::restore_with(data, recovery_kit, bucket, networking).await
    }

    pub(crate) async fn restore_with(
        data: &DataDir,
        recovery_kit: &str,
        bucket: Bucket,
        networking: Networking,
    ) -> Result<Self> {
        let kit = RecoveryKit::decode(recovery_kit)?;
        ensure_new_empty_dir(&data.path)?;
        let sealed = bucket
            .recovery_snapshot()
            .await?
            .context("no recovery snapshot in the bucket; the kit was never used")?;
        let snapshot = open_snapshot(&kit.recovery_key, &kit.workspace_id, &sealed)?;
        ensure_snapshot_is_current(&bucket, &kit, snapshot.epoch).await?;

        let restored = Self::install_snapshot(data, &kit, snapshot, bucket, networking).await;
        if restored.is_err() {
            // Leave nothing behind, so the restore can simply be retried.
            let _ = std::fs::remove_dir_all(&data.path);
            let _ = data.keys.forget(&data.path).await;
        }
        restored
    }

    async fn install_snapshot(
        data: &DataDir,
        kit: &RecoveryKit,
        snapshot: Snapshot,
        bucket: Bucket,
        networking: Networking,
    ) -> Result<Self> {
        std::fs::create_dir_all(&data.path)?;
        data.keys
            .save(&data.path, &DeviceKey::from_bytes(snapshot.device_key))
            .await?;
        write_private_file(&data.store_path(), &snapshot.store_db)?;
        write_private_file(&data.mls_path(), &snapshot.mls_db)?;
        write_private_file(&data.path.join(CONFIG_FILE), &snapshot.workspace_json)?;

        let workspace = Self::open_with(data, |_| Ok(bucket), networking).await?;
        ensure!(workspace.me() == kit.admin, "the snapshot is not the admin's");
        // Catch up before anyone writes, so our own sequence numbers continue
        // where the lost device left off.
        workspace.sync().await;
        if let Some(error) = workspace.status().error {
            anyhow::bail!("restored, but couldn't catch up from the bucket: {error}");
        }
        Ok(workspace)
    }
}

/// Restore writes several files; an empty (or new) folder keeps it clean.
fn ensure_new_empty_dir(dir: &std::path::Path) -> Result<()> {
    if dir.exists() {
        let is_empty = std::fs::read_dir(dir)?.next().is_none();
        ensure!(
            is_empty,
            "restore into a new, empty folder ({} isn't empty)",
            dir.display()
        );
    }
    Ok(())
}

/// A snapshot is stale if the admin committed after taking it: someone put
/// an old snapshot back, or the device died between commit and snapshot.
/// Restoring it would fork the group, so refuse.
async fn ensure_snapshot_is_current(bucket: &Bucket, kit: &RecoveryKit, epoch: u64) -> Result<()> {
    if let Some(bytes) = bucket.commit(epoch).await?
        && commit::verify(&bytes, epoch, &kit.admin).is_ok()
    {
        anyhow::bail!(
            "the workspace changed after this recovery snapshot was taken; restoring it would split the group"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_open_only_with_their_kit_and_workspace() {
        let snapshot = Snapshot {
            epoch: 0,
            device_key: [1; 32],
            workspace_json: b"{}".to_vec(),
            store_db: vec![2; 10],
            mls_db: vec![3; 10],
        };
        let sealed = seal_snapshot(&[9; 32], "ws", &snapshot).unwrap();
        assert_eq!(
            open_snapshot(&[9; 32], "ws", &sealed).unwrap().store_db,
            vec![2; 10]
        );
        assert!(open_snapshot(&[8; 32], "ws", &sealed).is_err());
        assert!(open_snapshot(&[9; 32], "other", &sealed).is_err());
    }
}
