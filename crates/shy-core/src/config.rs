//! A workspace's settings. Only non-secret fields go in `workspace.json`;
//! bucket credentials live in the encrypted store.

use std::{io::Write, path::Path};

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use crate::{ops::rule_violation, storage::Store, transport::bucket::BucketConfig};

pub(crate) const CONFIG_FILE: &str = "workspace.json";
const BUCKET_META: &str = "bucket_config";
/// The workspace id and admin, kept in the encrypted store as well as in the
/// plain workspace.json, so an edited file can't name a new admin.
const IDENTITY_META: &str = "workspace_identity";
const BUCKET_LAMPORT_META: &str = "bucket_config_lamport";

#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub workspace_id: String,
    pub name: String,
    pub admin: PublicKey,
    pub bucket: BucketConfig,
}

#[derive(Serialize, Deserialize)]
struct PublicSettings {
    workspace_id: String,
    name: String,
    admin: PublicKey,
}

impl WorkspaceConfig {
    pub fn exists(dir: &Path) -> bool {
        dir.join(CONFIG_FILE).exists()
    }

    /// The workspace's name, read without unlocking anything (it's public).
    pub fn peek_name(dir: &Path) -> Result<Option<String>> {
        let path = dir.join(CONFIG_FILE);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice::<PublicSettings>(&bytes)?.name)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn load(dir: &Path, store: &Store) -> Result<Self> {
        let path = dir.join(CONFIG_FILE);
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let settings: PublicSettings =
            serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
        match store.meta(IDENTITY_META)? {
            Some(kept) => {
                let (workspace_id, admin): (String, PublicKey) =
                    serde_json::from_slice(&kept).context("reading this workspace's id and admin")?;
                ensure!(
                    workspace_id == settings.workspace_id && admin == settings.admin,
                    "{} was changed outside the app: it no longer matches this workspace's encrypted settings",
                    path.display()
                );
            }
            // Workspaces from before the copy was kept take the file as it is once.
            None => store.set_meta(
                IDENTITY_META,
                &serde_json::to_vec(&(&settings.workspace_id, settings.admin))?,
            )?,
        }
        let bucket = store.meta(BUCKET_META)?.context("bucket settings are missing")?;
        Ok(Self {
            workspace_id: settings.workspace_id,
            name: settings.name,
            admin: settings.admin,
            bucket: serde_json::from_slice(&bucket)?,
        })
    }

    /// Keeps the bucket settings (syncing reads them there) and the
    /// workspace id and admin in the encrypted store.
    pub fn save_in_store(&self, store: &Store) -> Result<()> {
        store.set_meta(BUCKET_META, &serde_json::to_vec(&self.bucket)?)?;
        store.set_meta(
            IDENTITY_META,
            &serde_json::to_vec(&(&self.workspace_id, self.admin))?,
        )
    }

    /// Writes `workspace.json`, which marks the directory as a complete
    /// workspace, so it comes last.
    pub fn mark_complete(&self, dir: &Path) -> Result<()> {
        let settings = PublicSettings {
            workspace_id: self.workspace_id.clone(),
            name: self.name.clone(),
            admin: self.admin,
        };
        write_private_file(&dir.join(CONFIG_FILE), &serde_json::to_vec_pretty(&settings)?)
    }
}

/// The bucket settings in use now, which the admin may have changed since
/// the workspace was created.
pub(crate) fn current_bucket(store: &Store) -> Result<BucketConfig> {
    let bytes = store.meta(BUCKET_META)?.context("bucket settings are missing")?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// When the bucket settings were last changed (0 if never since creation).
pub(crate) fn bucket_lamport(store: &Store) -> Result<u64> {
    let Some(bytes) = store.meta(BUCKET_LAMPORT_META)? else {
        return Ok(0);
    };
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("corrupt settings clock"))?;
    Ok(u64::from_be_bytes(bytes))
}

/// Fails, as a broken rule, unless `bucket` is a change the admin may make:
/// only the region and keys, since the data stays at its endpoint and
/// bucket, and settings members can't connect with would lock them out.
pub(crate) fn check_settings_change(store: &Store, bucket: &BucketConfig) -> Result<()> {
    let current = current_bucket(store)?;
    if bucket.endpoint != current.endpoint || bucket.bucket != current.bucket {
        return Err(rule_violation(
            "storage settings that move the workspace elsewhere",
        ));
    }
    let is_filled = |value: &str| !value.trim().is_empty();
    if !is_filled(&bucket.region)
        || !is_filled(&bucket.access_key_id)
        || !is_filled(&bucket.secret_access_key)
    {
        return Err(rule_violation("storage settings without a region or keys"));
    }
    Ok(())
}

/// Applies new bucket settings unless newer ones (by lamport) are in place.
pub(crate) fn update_bucket(store: &Store, bucket: &BucketConfig, lamport: u64) -> Result<bool> {
    if lamport <= bucket_lamport(store)? {
        return Ok(false);
    }
    store.set_meta(BUCKET_META, &serde_json::to_vec(bucket)?)?;
    store.set_meta(BUCKET_LAMPORT_META, &lamport.to_be_bytes())?;
    Ok(true)
}

/// Writes a file only its owner can read.
/// Written beside it and renamed over it, so a crash leaves the old file or
/// the new one, never half of one.
pub(crate) fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let file_name = path.file_name().context("writing a file without a name")?;
    let partial = path.with_file_name(format!("{}.partial", file_name.to_string_lossy()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
        .open(&partial)
        .and_then(|mut file| {
            file.write_all(contents)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&partial, path))
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_file_is_replaced_whole_and_only_its_owner_reads_it() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("workspace.json");
        write_private_file(&path, b"first").unwrap();
        write_private_file(&path, b"second").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let names: Vec<_> = std::fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            ["workspace.json"],
            "nothing half written is left beside it"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
