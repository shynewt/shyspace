//! The local SQLite database: the op log, epoch keys and membership, and the
//! views (channels, messages, pages, search) that ops are folded into.

mod apply;
mod epochs;
mod invites;
mod oplog;
mod queries;
mod schema;

pub use invites::PendingRequest;

use std::path::Path;

use anyhow::{Context, Result};
use iroh::PublicKey;
use rusqlite::{Connection, OptionalExtension, Params, params};

/// A fixed key for databases that only live inside tests.
#[cfg(test)]
pub(crate) const TEST_DB_KEY: &str = "x'0000000000000000000000000000000000000000000000000000000000000000'";

pub struct Store {
    db: Connection,
}

impl Store {
    /// Opens (or creates) the encrypted database at `path`. `key` is a
    /// SQLCipher raw-key literal from [`crate::keys::DeviceKey`].
    pub fn open(path: &Path, key: &str) -> Result<Self> {
        let mut db = open_encrypted(path, key)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        schema::migrate(&mut db)?;
        Ok(Self { db })
    }

    /// Runs `work` atomically: either everything it wrote is kept, or nothing.
    pub fn atomically<T>(&self, work: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let transaction = self.db.unchecked_transaction()?;
        let value = work(self)?;
        transaction.commit()?;
        Ok(value)
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
