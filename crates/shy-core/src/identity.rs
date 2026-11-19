//! A member's identity: one ed25519 key that is their network address, signs
//! their ops, and names them in the MLS group.

use std::path::Path;

use anyhow::{Context, Result};
use iroh::SecretKey;

use crate::config::write_private_file;

const IDENTITY_FILE: &str = "identity.key";

pub fn load_or_create(dir: &Path) -> Result<SecretKey> {
    let path = dir.join(IDENTITY_FILE);
    if path.exists() {
        let bytes: [u8; 32] = std::fs::read(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("{} is corrupt", path.display()))?;
        return Ok(SecretKey::from_bytes(&bytes));
    }
    let identity = SecretKey::generate();
    write_private_file(&path, &identity.to_bytes())?;
    Ok(identity)
}
