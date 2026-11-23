//! Admin recovery. If the admin's device is lost, membership could never change
//! again. So the admin keeps a recovery kit (a code to store in a password
//! manager), and after every membership change their app uploads an encrypted
//! snapshot of its admin state to the bucket. Restoring on a new device needs
//! only the kit; messages and pages come back from the bucket as usual.
//!
//! Only restore when the old device is truly gone: two live copies of the
//! admin would make conflicting membership changes.

use anyhow::{Context, Result, anyhow, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};
use tracing::warn;
use zeroize::Zeroizing;

use super::commit;
use crate::{
    config::{CONFIG_FILE, write_private_file},
    crypto,
    keys::DeviceKey,
    storage::{snapshot_database, snapshot_store_without_content},
    transport::bucket::{Bucket, BucketConfig, Connector, MAX_RECOVERY_SNAPSHOT_BYTES, s3_connector},
    workspace::{DataDir, MLS_FILE, STORE_FILE, Workspace, WorkspaceOptions},
};

const KIT_PREFIX: &str = "shyrec1_";
const RECOVERY_KEY_META: &str = "recovery_key";
/// Bumped on every membership change; the snapshot is current when the
/// uploaded generation matches.
const CHANGE_GENERATION_META: &str = "recovery_change_generation";
const UPLOADED_GENERATION_META: &str = "recovery_uploaded_generation";
/// The latest generation whose snapshot was too large to upload.
const OVERSIZED_GENERATION_META: &str = "recovery_oversized_generation";
/// How large the last snapshot was, uploaded or not.
const SNAPSHOT_BYTES_META: &str = "recovery_snapshot_bytes";
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

/// The workspace a recovery kit restores, read without restoring anything.
pub(crate) fn kit_workspace_id(recovery_kit: &str) -> Result<String> {
    Ok(RecoveryKit::decode(recovery_kit)?.workspace_id)
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
    pub fn has_recovery_kit(&self) -> Result<bool> {
        Ok(self.state().store.meta(RECOVERY_KEY_META)?.is_some())
    }

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
        {
            let store = &self.state().store;
            store.delete_meta(UPLOADED_GENERATION_META)?;
            store.delete_meta(OVERSIZED_GENERATION_META)?;
        }
        self.refresh_recovery_snapshot().await?;
        ensure!(
            self.state().store.meta(OVERSIZED_GENERATION_META)?.is_none(),
            "the recovery snapshot is too large for restore to read ({} MB at most), so a recovery kit can't restore this workspace",
            MAX_RECOVERY_SNAPSHOT_BYTES / (1024 * 1024)
        );
        let config = &self.inner.config;
        RecoveryKit {
            workspace_id: config.workspace_id.clone(),
            admin: config.admin,
            bucket: self.bucket_config()?,
            recovery_key: *recovery_key,
        }
        .encode()
    }

    /// How large the last recovery snapshot was, if this device took one.
    pub(crate) fn recovery_snapshot_bytes(store: &crate::storage::Store) -> Result<Option<u64>> {
        let Some(bytes) = store.meta(SNAPSHOT_BYTES_META)? else {
            return Ok(None);
        };
        let bytes: [u8; 8] = bytes
            .try_into()
            .map_err(|_| anyhow!("the stored recovery snapshot size is corrupt"))?;
        Ok(Some(u64::from_be_bytes(bytes)))
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
            let is_done = |meta: &str| -> Result<bool> {
                Ok(state.store.meta(meta)?.is_some() && generation(&state.store, meta)? == current)
            };
            if is_done(UPLOADED_GENERATION_META)? || is_done(OVERSIZED_GENERATION_META)? {
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
        let size = sealed.len() as u64;
        self.state()
            .store
            .set_meta(SNAPSHOT_BYTES_META, &size.to_be_bytes())?;
        if size > MAX_RECOVERY_SNAPSHOT_BYTES {
            // Failing here would hold back the commit that changed membership.
            // The kit keeps its older snapshot, which restore then refuses as
            // stale rather than splitting the group.
            warn!(
                "the recovery snapshot ({} bytes) is too large for restore to read; the recovery kit can't restore this workspace",
                sealed.len()
            );
            return self
                .state()
                .store
                .set_meta(OVERSIZED_GENERATION_META, &generation_taken.to_be_bytes());
        }
        self.bucket().put_recovery_snapshot(sealed).await?;
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
    pub async fn restore(
        data: &DataDir,
        recovery_kit: &str,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Self> {
        Self::restore_with(data, recovery_kit, s3_connector(), options).await
    }

    pub(crate) async fn restore_with(
        data: &DataDir,
        recovery_kit: &str,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Self> {
        let restored = Self::restore_without_catching_up(data, recovery_kit, connect, options).await?;
        // Our own log must be read back, so our sequence numbers continue
        // where the lost device left off; what's left of others' logs is read
        // like on any other day, and one unreadable object there can't block
        // the restore.
        restored.sync().await;
        let checked = restored.state().store.is_catching_up();
        let is_catching_up = match checked {
            Ok(is_catching_up) => is_catching_up,
            Err(error) => {
                Self::undo_restore(data, restored).await;
                return Err(error.context("checking the restore caught up"));
            }
        };
        if is_catching_up {
            let reason = restored.status().error.unwrap_or_default();
            Self::undo_restore(data, restored).await;
            return Err(anyhow!(
                "restored, but couldn't catch up with this device's own changes in the bucket: {reason}"
            ));
        }
        Ok(restored)
    }

    /// Restores the snapshot into `data` and opens it, without reading the
    /// bucket yet. Undone if it fails.
    pub(crate) async fn restore_without_catching_up(
        data: &DataDir,
        recovery_kit: &str,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Self> {
        let kit = RecoveryKit::decode(recovery_kit)?;
        let bucket = connect(&kit.workspace_id, &kit.bucket)?;
        ensure_new_empty_dir(&data.path)?;
        let sealed = bucket
            .recovery_snapshot()
            .await?
            .context("no recovery snapshot in the bucket; the kit was never used")?;
        let snapshot = open_snapshot(&kit.recovery_key, &kit.workspace_id, &sealed)?;
        ensure_snapshot_is_current(&bucket, &kit, snapshot.epoch).await?;

        std::fs::create_dir_all(&data.path)?;
        if let Err(error) = data
            .keys
            .save(&data.path, &DeviceKey::from_bytes(snapshot.device_key))
            .await
        {
            // Nothing of ours is in the folder yet; the key slot was taken by
            // something else, which we must not touch.
            let _ = std::fs::remove_dir(&data.path);
            return Err(error);
        }
        let restored = Self::install_snapshot(data, &kit, snapshot, connect, options).await;
        if restored.is_err() {
            Self::forget_restored_files(data).await;
        }
        restored
    }

    async fn install_snapshot(
        data: &DataDir,
        kit: &RecoveryKit,
        snapshot: Snapshot,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Self> {
        write_private_file(&data.store_path(), &snapshot.store_db)?;
        write_private_file(&data.mls_path(), &snapshot.mls_db)?;
        write_private_file(&data.path.join(CONFIG_FILE), &snapshot.workspace_json)?;

        let workspace = Self::open_with(data, connect, options).await?;
        ensure!(workspace.me() == kit.admin, "the snapshot is not the admin's");
        Ok(workspace)
    }

    /// Once this device's own log is read back after a restore, writes what
    /// opening held back; until then nothing is written. A restore cut short
    /// finishes here too, on the next sync.
    pub(crate) async fn finish_catching_up(&self) -> Result<()> {
        if !self.state().store.is_catching_up()? {
            return Ok(());
        }
        let me = self.me();
        self.forget_skipped_segments_of(&me);
        if let Some(error) = self.pull_author(&me).await? {
            return Err(error.context("reading this device's changes back after a restore"));
        }
        if let Some(seq) = self.parked_own_change()? {
            bail!(
                "this device's change {seq} came back from the bucket, but this version can't apply it yet; update shyspace"
            );
        }
        self.state().store.mark_caught_up()?;
        self.finish_writes_left_over()
    }

    /// The seq of one of our own changes that came back but waits, unapplied
    /// (one from a newer version, say): a write now would take its place.
    /// Only one we signed counts, so nobody can plant one to hold us up.
    fn parked_own_change(&self) -> Result<Option<u64>> {
        let me = self.me();
        let state = self.state();
        let applied_through = state.store.last_seq(&me)?;
        for envelope in state.store.pending()? {
            if envelope.author != me || envelope.seq <= applied_through {
                continue;
            }
            let Some(key) = state.store.epoch_key(envelope.epoch)? else {
                continue;
            };
            if crypto::signed_body(&envelope, &key).is_ok() {
                return Ok(Some(envelope.seq));
            }
        }
        Ok(None)
    }

    /// Closes a restore that failed and removes all it did, so it can simply
    /// be tried again. Its background work stops first, and a sync pass under
    /// way is waited out, so nothing of it carries on beside the retry.
    async fn undo_restore(data: &DataDir, restored: Self) {
        if let Err(error) = restored.shutdown().await {
            warn!("closing a restore that failed: {error:#}");
        }
        drop(restored.inner.sync_lock.lock().await);
        drop(restored);
        Self::forget_restored_files(data).await;
    }

    /// The key goes first: its keychain slot is named after the folder.
    async fn forget_restored_files(data: &DataDir) {
        let _ = data.keys.forget(&data.path).await;
        let _ = std::fs::remove_dir_all(&data.path);
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
    // Every later slot counts: deleting just the next one mustn't make a
    // stale snapshot look current.
    for later in bucket
        .commit_epochs()
        .await?
        .into_iter()
        .filter(|slot| *slot >= epoch)
    {
        if let Some(bytes) = bucket.commit(later).await?
            && commit::verify(&bytes, later, &kit.admin).is_ok()
        {
            anyhow::bail!(
                "the workspace changed after this recovery snapshot was taken; restoring it would split the group"
            );
        }
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
