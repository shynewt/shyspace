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

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
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

fn to_public_key(bytes: Vec<u8>) -> Result<PublicKey> {
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("stored member id is not 32 bytes"))?;
    Ok(PublicKey::from_bytes(&bytes)?)
}
