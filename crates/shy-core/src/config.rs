//! A workspace's settings. Only non-secret fields go in `workspace.json`;
//! bucket credentials live in the encrypted store.

use std::{io::Write, path::Path};

use anyhow::{Context, Result};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use crate::{storage::Store, transport::bucket::BucketConfig};

pub(crate) const CONFIG_FILE: &str = "workspace.json";
const BUCKET_META: &str = "bucket_config";

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

    pub fn load(dir: &Path, store: &Store) -> Result<Self> {
        let path = dir.join(CONFIG_FILE);
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let settings: PublicSettings =
            serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
        let bucket = store.meta(BUCKET_META)?.context("bucket settings are missing")?;
        Ok(Self {
            workspace_id: settings.workspace_id,
            name: settings.name,
            admin: settings.admin,
            bucket: serde_json::from_slice(&bucket)?,
        })
    }

    /// Saves the settings. Writing `workspace.json` last marks the directory
    /// as a complete workspace.
    pub fn save(&self, dir: &Path, store: &Store) -> Result<()> {
        store.set_meta(BUCKET_META, &serde_json::to_vec(&self.bucket)?)?;
        let settings = PublicSettings {
            workspace_id: self.workspace_id.clone(),
            name: self.name.clone(),
            admin: self.admin,
        };
        write_private_file(&dir.join(CONFIG_FILE), &serde_json::to_vec_pretty(&settings)?)
    }
}

/// Writes a file only its owner can read.
pub(crate) fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
        .open(path)
        .and_then(|mut file| file.write_all(contents))
        .with_context(|| format!("writing {}", path.display()))
}
