//! Epoch keys, who belonged to each epoch, and the admin's records of
//! commits and Welcomes still to deliver.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::{Store, to_public_key};
use crate::crypto::EpochKey;

impl Store {
    /// Remembers an epoch's key and who was a member during it. An op is only
    /// accepted if its author was a member of the op's epoch.
    pub fn put_epoch(&self, epoch: u64, key: &EpochKey, members: &[PublicKey]) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO epoch_keys (epoch, key) VALUES (?, ?)",
            params![epoch, key.as_slice()],
        )?;
        for member in members {
            self.db.execute(
                "INSERT OR IGNORE INTO epoch_members (epoch, member) VALUES (?, ?)",
                params![epoch, member.as_bytes()],
            )?;
        }
        Ok(())
    }

    pub fn epoch_key(&self, epoch: u64) -> Result<Option<EpochKey>> {
        let key: Option<Vec<u8>> = self
            .db
            .query_row("SELECT key FROM epoch_keys WHERE epoch = ?", [epoch], |row| {
                row.get(0)
            })
            .optional()?;
        key.map(to_epoch_key).transpose()
    }

    pub fn epoch_keys(&self) -> Result<Vec<(u64, EpochKey)>> {
        let mut statement = self
            .db
            .prepare("SELECT epoch, key FROM epoch_keys ORDER BY epoch")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
        rows.map(|row| {
            let (epoch, key) = row?;
            Ok((epoch, to_epoch_key(key)?))
        })
        .collect()
    }

    pub fn epoch_members(&self, epoch: u64) -> Result<Vec<PublicKey>> {
        let mut statement = self
            .db
            .prepare("SELECT member FROM epoch_members WHERE epoch = ?")?;
        let rows = statement.query_map([epoch], |row| row.get::<_, Vec<u8>>(0))?;
        rows.map(|bytes| to_public_key(bytes?)).collect()
    }

    pub fn was_member_at(&self, epoch: u64, member: &PublicKey) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM epoch_members WHERE epoch = ? AND member = ?",
            params![epoch, member.as_bytes()],
        )
    }

    pub fn was_ever_member(&self, member: &PublicKey) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM epoch_members WHERE member = ? LIMIT 1",
            [member.as_bytes()],
        )
    }

    /// Members ever seen stay listed (so old messages keep a name), but only
    /// current MLS members are active.
    pub fn set_current_members(&self, current: &[PublicKey]) -> Result<()> {
        self.db.execute("UPDATE members SET active = 0", [])?;
        for member in current {
            self.db.execute(
                "INSERT INTO members (member, active) VALUES (?, 1) ON CONFLICT (member) DO UPDATE SET active = 1",
                [member.as_bytes()],
            )?;
        }
        Ok(())
    }

    pub fn put_commit_record(&self, from_epoch: u64, signed: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO commits (from_epoch, signed) VALUES (?, ?)",
            params![from_epoch, signed],
        )?;
        Ok(())
    }

    pub fn has_commit_record(&self, from_epoch: u64) -> Result<bool> {
        self.exists("SELECT 1 FROM commits WHERE from_epoch = ?", [from_epoch])
    }

    pub fn commit_records(&self) -> Result<Vec<(u64, Vec<u8>)>> {
        let mut statement = self
            .db
            .prepare("SELECT from_epoch, signed FROM commits ORDER BY from_epoch")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn put_delivery(&self, joiner: &PublicKey, payload: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO deliveries (joiner, payload) VALUES (?, ?)",
            params![joiner.as_bytes(), payload],
        )?;
        Ok(())
    }

    pub fn deliveries(&self) -> Result<Vec<(PublicKey, Vec<u8>)>> {
        let mut statement = self.db.prepare("SELECT joiner, payload FROM deliveries")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        rows.map(|row| {
            let (joiner, payload) = row?;
            Ok((to_public_key(joiner)?, payload))
        })
        .collect()
    }

    pub fn delete_delivery(&self, joiner: &PublicKey) -> Result<()> {
        self.db
            .execute("DELETE FROM deliveries WHERE joiner = ?", [joiner.as_bytes()])?;
        Ok(())
    }
}

fn to_epoch_key(bytes: Vec<u8>) -> Result<EpochKey> {
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("stored epoch key has the wrong length"))
}
