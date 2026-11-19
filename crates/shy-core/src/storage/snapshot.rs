//! Consistent copies of the encrypted databases, for the admin's recovery
//! snapshot. Copies keep the source's encryption.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::backup::Backup;

use super::open_encrypted;

/// Tables rebuilt from the bucket after a restore, so they're left out of
/// snapshots to keep them small.
const REPLAYABLE_TABLES: &[&str] = &[
    "ops",
    "pending_ops",
    "cursors",
    "profiles",
    "channels",
    "messages",
    "reactions",
    "pages",
    "search",
    "message_edits",
    "channel_names",
    "deleted",
];

/// Copies the database at `source` to `destination`, both under `key`.
pub fn snapshot_database(source: &Path, key: &str, destination: &Path) -> Result<()> {
    let from = open_encrypted(source, key)?;
    let mut to = open_encrypted(destination, key)?;
    Backup::new(&from, &mut to)?
        .run_to_completion(256, std::time::Duration::ZERO, None)
        .with_context(|| format!("copying {}", source.display()))
}

/// Like [`snapshot_database`] for the main store, minus everything the bucket
/// can give back.
pub fn snapshot_store_without_content(source: &Path, key: &str, destination: &Path) -> Result<()> {
    snapshot_database(source, key, destination)?;
    let copy = open_encrypted(destination, key)?;
    for table in REPLAYABLE_TABLES {
        copy.execute(&format!("DELETE FROM {table}"), [])?;
    }
    copy.execute_batch("VACUUM")?;
    Ok(())
}
