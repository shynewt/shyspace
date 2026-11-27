//! The local SQLite database: the op log, epoch keys and membership, and the
//! views (channels, messages, pages, search) that ops are folded into.

mod apply;
mod device_changes;
mod devices;
mod epochs;
mod files;
mod forks;
mod invites;
mod later;
mod look;
mod oplog;
mod page_cache;
mod page_mentions;
mod private;
mod private_members;
mod profile_details;
mod queries;
mod reading;
mod rebuild;
mod revisions;
mod rights;
mod schema;
mod sealed;
mod sealing;
mod search_index;
mod sections;
mod sent;
mod snapshot;
#[cfg(test)]
mod test_channel;
mod threads;

pub use apply::{Applied, ApplyContext};
pub use device_changes::RecordedChange;
pub use devices::DeviceSource;
pub(crate) use devices::MAX_DEVICES_PER_PERSON;
pub use files::{BlobRemoval, StoredFile};
pub(crate) use forks::CheckedFork;
pub use invites::PendingRequest;
pub use page_mentions::PageMentionSpot;
pub use private::{Origin, StoredKey};
pub use private_members::{Ending, Membership};
pub use queries::SearchFilters;
pub use reading::UnreadMention;
pub use revisions::ItemKind;
pub(crate) use schema::{VIEW_RULES, VIEW_RULES_META};
pub use sealed::WaitingSealed;
pub use sections::BUILTIN_SECTION;
pub use snapshot::{snapshot_database, snapshot_store_without_content};
pub use threads::Reply;

use std::path::Path;

use anyhow::{Context, Result};
use iroh::PublicKey;
use rusqlite::{Connection, OptionalExtension, Params, params};
use tracing::warn;

/// A fixed key for databases that only live inside tests.
#[cfg(test)]
impl Store {
    /// Runs raw SQL, to set up states a test needs.
    pub(crate) fn execute_for_test(&self, sql: &str) {
        self.db.execute_batch(sql).expect("test SQL");
    }

    /// Counts the page rows changed from now on, a measure of page writes
    /// that doesn't depend on the machine.
    pub(crate) fn count_page_writes(&self) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = count.clone();
        self.db.update_hook(Some(move |action, _: &str, table: &str, _| {
            if table == "pages" && action == rusqlite::hooks::Action::SQLITE_UPDATE {
                counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }));
        count
    }

    /// How many rows of `table` have `column` equal to `value`.
    pub(crate) fn count_where(&self, table: &str, column: &str, value: &str) -> i64 {
        self.db
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE {column} = ?"),
                [value],
                |row| row.get(0),
            )
            .expect("counting rows")
    }

    pub(crate) fn table_names(&self) -> Vec<String> {
        self.db
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .expect("listing tables")
            .query_map([], |row| row.get(0))
            .expect("listing tables")
            .collect::<rusqlite::Result<_>>()
            .expect("listing tables")
    }

    /// SQLite steps run by `work`, a measure of its cost that doesn't depend
    /// on the machine.
    pub(crate) fn steps_of(&self, work: impl FnOnce()) -> u64 {
        use std::sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        };
        let steps = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&steps);
        self.db.progress_handler(
            1,
            Some(move || {
                counter.fetch_add(1, Ordering::Relaxed);
                false
            }),
        );
        work();
        self.db.progress_handler(1, None::<fn() -> bool>);
        steps.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
pub(crate) const TEST_DB_KEY: &str = "x'0000000000000000000000000000000000000000000000000000000000000000'";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Settling {
    Nothing,
    OwnerOps,
    Leaves,
    Everything,
}

/// Set in every recovery snapshot's store; see [`Store::is_catching_up`].
pub(crate) const CATCHING_UP_META: &str = "catching_up_after_restore";

pub struct Store {
    db: Connection,
    /// Which sealed ops may apply now. A rebuild settles the owner logs first,
    /// then leaves, then content, so each is judged against settled state
    /// rather than the replay's order.
    pub(crate) settling: std::cell::Cell<Settling>,
    pending_pages: std::cell::RefCell<page_cache::PendingPages>,
}

impl Store {
    /// Opens (or creates) the encrypted database at `path`. `key` is a
    /// SQLCipher raw-key literal from [`crate::keys::DeviceKey`].
    pub fn open(path: &Path, key: &str) -> Result<Self> {
        let mut db = open_encrypted(path, key)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        schema::create_or_migrate(&mut db)?;
        Ok(Self {
            db,
            settling: std::cell::Cell::new(Settling::Everything),
            pending_pages: std::cell::RefCell::default(),
        })
    }

    pub(crate) fn is_in_transaction(&self) -> bool {
        !self.db.is_autocommit()
    }

    /// Runs `work` atomically: either everything it wrote is kept, or nothing.
    /// Inside a transaction already, `work` runs in a savepoint of it.
    pub fn atomically<T>(&self, work: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        if !self.db.is_autocommit() {
            return self.with_savepoint(work);
        }
        let transaction = self.db.unchecked_transaction()?;
        let outcome = work(self).and_then(|value| {
            self.write_pending_pages()?;
            Ok(value)
        });
        if outcome.is_err() {
            self.forget_pending_pages();
        }
        let value = outcome?;
        transaction.commit()?;
        Ok(value)
    }

    /// Runs `work` inside the current transaction so that, if it fails,
    /// none of its writes remain.
    pub(crate) fn with_savepoint<T>(&self, work: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        if self.db.is_autocommit() {
            return self.atomically(work);
        }
        self.db.execute_batch("SAVEPOINT work")?;
        self.pending_pages.borrow_mut().begin_savepoint();
        match work(self) {
            Ok(value) => {
                if let Err(error) = self.db.execute_batch("RELEASE work") {
                    self.pending_pages.borrow_mut().lose_track();
                    return Err(error.into());
                }
                self.pending_pages.borrow_mut().release_savepoint();
                Ok(value)
            }
            // SQLite may have given up the whole transaction itself (a full
            // disk, say); then there's no savepoint left to go back to.
            Err(error) if self.db.is_autocommit() => {
                self.forget_pending_pages();
                Err(error)
            }
            Err(error) => {
                let rolled_back = self
                    .db
                    .execute_batch("ROLLBACK TO work; RELEASE work")
                    .map_err(anyhow::Error::from)
                    .and_then(|()| self.pending_pages.borrow_mut().roll_back_savepoint());
                if let Err(failure) = rolled_back {
                    self.pending_pages.borrow_mut().lose_track();
                    warn!("couldn't take back a failed step: {failure:#}");
                }
                Err(error)
            }
        }
    }

    pub fn meta(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .query_row("SELECT value FROM meta WHERE key = ?", [key], |row| row.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_meta(&self, key: &str) -> Result<()> {
        self.db.execute("DELETE FROM meta WHERE key = ?", [key])?;
        Ok(())
    }

    /// Restored from a snapshot, without this device's own log read back
    /// yet: writing now would reuse sequence numbers the bucket holds.
    pub fn is_catching_up(&self) -> Result<bool> {
        Ok(self.meta(CATCHING_UP_META)?.is_some())
    }

    pub fn mark_caught_up(&self) -> Result<()> {
        self.delete_meta(CATCHING_UP_META)
    }

    fn exists(&self, sql: &str, args: impl Params) -> Result<bool> {
        Ok(self.db.query_row(sql, args, |_| Ok(())).optional()?.is_some())
    }
}

/// Opens a SQLCipher database and checks the key actually decrypts it.
pub(crate) fn open_encrypted(path: &Path, key: &str) -> Result<Connection> {
    let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    db.pragma_update(None, "key", key)?;
    db.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .with_context(|| format!("{} can't be decrypted with this device's key", path.display()))?;
    Ok(db)
}

fn to_public_key(bytes: Vec<u8>) -> Result<PublicKey> {
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("stored member id is not 32 bytes"))?;
    Ok(PublicKey::from_bytes(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failing_step_inside_a_transaction_undoes_only_its_own_writes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();

        store
            .atomically(|outer| {
                outer.set_meta("kept", b"yes")?;
                let inner = outer.atomically(|inner| -> Result<()> {
                    inner.set_meta("undone", b"yes")?;
                    anyhow::bail!("this step fails")
                });
                assert!(inner.is_err());
                outer.set_meta("also kept", b"yes")
            })
            .unwrap();

        assert!(store.meta("kept").unwrap().is_some() && store.meta("also kept").unwrap().is_some());
        assert_eq!(store.meta("undone").unwrap(), None);
    }

    /// SQLite can give up a whole transaction itself (a full disk, say);
    /// the step's own error is what's reported, and nothing goes on as if
    /// the transaction were still there.
    #[test]
    fn a_transaction_sqlite_gave_up_reports_the_real_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();

        let outcome = store.atomically(|outer| {
            let step = outer.atomically(|inner| -> Result<()> {
                inner.execute_for_test("ROLLBACK");
                anyhow::bail!("the disk is full")
            });
            assert!(!outer.is_in_transaction(), "the transaction is gone");
            step
        });

        let error = format!("{:#}", outcome.unwrap_err());
        assert!(error.contains("the disk is full"), "{error}");
    }
}
