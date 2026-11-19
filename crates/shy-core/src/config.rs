//! Files in a member's data directory.

use std::{io::Write, path::Path};

use anyhow::{Context, Result};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use crate::transport::bucket::BucketConfig;

const CONFIG_FILE: &str = "workspace.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub workspace_id: String,
    pub name: String,
    pub admin: PublicKey,
    pub bucket: BucketConfig,
}

impl WorkspaceConfig {
    pub fn exists(dir: &Path) -> bool {
        dir.join(CONFIG_FILE).exists()
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(CONFIG_FILE);
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        write_private_file(&dir.join(CONFIG_FILE), &serde_json::to_vec_pretty(self)?)
    }
}

/// Writes a file only its owner can read: it holds keys or credentials.
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
