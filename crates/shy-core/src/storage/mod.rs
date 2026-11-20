//! The local SQLite database: the op log, epoch keys and membership, and the
//! views (channels, messages, pages, search) that ops are folded into.

mod apply;
mod epochs;
mod invites;
mod later;
mod oplog;
mod private;
mod private_members;
mod queries;
mod reading;
mod rebuild;
mod revisions;
mod schema;
mod sealed;
mod sealing;
mod sections;
mod snapshot;
mod threads;

pub use apply::{Applied, ApplyContext};
pub use invites::PendingRequest;
pub use private::{Origin, StoredKey};
pub use private_members::{Ending, Membership};
pub use queries::SearchFilters;
pub use reading::UnreadMessage;
pub use revisions::ItemKind;
pub use sealed::WaitingSealed;
pub use sections::BUILTIN_SECTION;
pub use snapshot::{snapshot_database, snapshot_store_without_content};
pub use threads::Reply;

use std::path::Path;

use anyhow::{Context, Result};
use iroh::PublicKey;
use rusqlite::{Connection, OptionalExtension, Params, params};

/// A fixed key for databases that only live inside tests.
#[cfg(test)]
impl Store {
    /// Runs raw SQL, to set up states older versions left behind.
    pub(crate) fn execute_for_test(&self, sql: &str) {
        self.db.execute_batch(sql).expect("test SQL");
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

pub struct Store {
    db: Connection,
    /// Which sealed ops may apply now. A rebuild settles the owner logs first,
    /// then leaves, then content, so each is judged against settled state
    /// rather than the replay's order.
    pub(crate) settling: std::cell::Cell<Settling>,
}

impl Store {
    /// Opens (or creates) the encrypted database at `path`. `key` is a
    /// SQLCipher raw-key literal from [`crate::keys::DeviceKey`].
    pub fn open(path: &Path, key: &str) -> Result<Self> {
        let mut db = open_encrypted(path, key)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        schema::migrate(&mut db)?;
        Ok(Self {
            db,
            settling: std::cell::Cell::new(Settling::Everything),
        })
    }

    /// Runs `work` atomically: either everything it wrote is kept, or nothing.
    pub fn atomically<T>(&self, work: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let transaction = self.db.unchecked_transaction()?;
        let value = work(self)?;
        transaction.commit()?;
        Ok(value)
    }

    /// Runs `work` inside the current transaction so that, if it fails,
    /// none of its writes remain.
    pub(crate) fn with_savepoint<T>(&self, work: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        self.db.execute_batch("SAVEPOINT work")?;
        match work(self) {
            Ok(value) => {
                self.db.execute_batch("RELEASE work")?;
                Ok(value)
            }
            Err(error) => {
                self.db.execute_batch("ROLLBACK TO work; RELEASE work")?;
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
