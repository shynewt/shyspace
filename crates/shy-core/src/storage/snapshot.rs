//! Consistent copies of the encrypted databases, for the admin's recovery
//! snapshot. Copies keep the source's encryption.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::backup::Backup;

use super::{open_encrypted, private::PRIVATE_STATE_TABLES, rebuild::SHARED_VIEW_TABLES};

/// The op log and how far we've read the bucket: a restore reads them again.
const OP_LOG_TABLES: &[&str] = &["ops", "pending_ops", "forks", "fork_proofs_checked"];

/// This device's own state (read positions, alerts, follows, drafts,
/// Later), which a new device starts without. The sidebar's sections and
/// which channel sits in each are kept together, so they come back as one.
const PERSONAL_TABLES: &[&str] = &[
    "read_markers",
    "channel_notify",
    "thread_follows",
    "drafts",
    "later",
];

/// Everything a snapshot leaves out: what replaying the bucket gives back
/// (the op log, the views, private channel state), queued file removals,
/// which a rebuild starts without too, and this device's personal state.
fn tables_left_out() -> impl Iterator<Item = &'static str> {
    OP_LOG_TABLES
        .iter()
        .chain(SHARED_VIEW_TABLES)
        .chain(PRIVATE_STATE_TABLES)
        .chain(PERSONAL_TABLES)
        .chain(&["blob_removals"])
        .copied()
}

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
    for table in tables_left_out() {
        copy.execute(&format!("DELETE FROM {table}"), [])?;
    }
    copy.execute_batch("VACUUM")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Store, TEST_DB_KEY};

    /// What a snapshot keeps: the keys, the membership and its history,
    /// invites and their pending welcomes, settings, and the admin's sidebar.
    const TABLES_KEPT: &[&str] = &[
        "commits",
        "deliveries",
        "epoch_keys",
        "epoch_members",
        "frozen_authors",
        "invites",
        "members",
        "meta",
        "sealing_secrets",
        "sidebar_placement",
        "sidebar_sections",
    ];

    /// FTS5 keeps the search index in tables of its own.
    const SEARCH_INDEX_PARTS: &[&str] = &[
        "search_config",
        "search_content",
        "search_data",
        "search_docsize",
        "search_idx",
    ];

    #[test]
    fn every_table_is_kept_or_left_out_of_snapshots_and_every_listed_one_exists() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        let left_out: Vec<&str> = tables_left_out().collect();
        let mut unsorted = Vec::new();
        for table in store.table_names() {
            let places = usize::from(left_out.contains(&table.as_str()))
                + usize::from(TABLES_KEPT.contains(&table.as_str()))
                + usize::from(SEARCH_INDEX_PARTS.contains(&table.as_str()));
            if places != 1 {
                unsorted.push(table);
            }
        }
        assert!(unsorted.is_empty(), "in no list, or in two: {unsorted:?}");
        let tables = store.table_names();
        let missing: Vec<&str> = left_out
            .iter()
            .chain(TABLES_KEPT)
            .filter(|listed| !tables.iter().any(|table| table == *listed))
            .copied()
            .collect();
        assert!(missing.is_empty(), "listed but not tables: {missing:?}");
    }
}
