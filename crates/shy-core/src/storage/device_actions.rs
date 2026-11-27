//! The admin's own record of acting on device changes: which it acted on,
//! and the work each still owes (a welcome to deliver, an outcome to upload,
//! a key-only commit). None of it is a view, so a rebuild can't make the
//! admin act twice, and the recovery snapshot keeps it. The acted mark and
//! the work it owes are written in one transaction.

use anyhow::{Context, Result};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::{Store, device_changes::RecordedChange, to_public_key};
use crate::membership::{
    device::{BoundFreeze, DeviceRecord},
    link::Token,
};

const KEY_ONLY_COMMIT_OWED_META: &str = "key_only_commit_owed";

/// What the admin's app did with a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acted {
    Added,
    Removed,
    Refused,
    /// A retry is owed by the voucher, not the admin.
    Stale,
    /// Nothing to do: already done, or its author was gone by then.
    Skipped,
}

impl Acted {
    fn from_str(stored: &str) -> Result<Self> {
        Ok(match stored {
            "added" => Self::Added,
            "removed" => Self::Removed,
            "refused" => Self::Refused,
            "stale" => Self::Stale,
            "skipped" => Self::Skipped,
            _ => anyhow::bail!("an unknown stored device action: {stored}"),
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Refused => "refused",
            Self::Stale => "stale",
            Self::Skipped => "skipped",
        }
    }
}

/// A welcome owed to a device the admin added. The sealed grant is made once
/// the add's epoch is recorded, from the map as it stood before the add.
pub struct OwedWelcome {
    pub device: PublicKey,
    pub token: Token,
    pub welcome: Vec<u8>,
    pub map_digest: [u8; 32],
    /// The epoch the add moved to: the only one the grant can be sealed in.
    pub epoch: u64,
    pub devices_before: Vec<DeviceRecord>,
    pub bound_frozen_before: Vec<BoundFreeze>,
    pub sealed_grant: Option<Vec<u8>>,
    pub until_ms: i64,
}

impl Store {
    /// Changes the admin hasn't acted on, in the order this device applied
    /// them.
    pub fn unacted_device_changes(&self) -> Result<Vec<RecordedChange>> {
        let acted: Vec<(Vec<u8>, u64)> = {
            let mut statement = self.db.prepare("SELECT author, seq FROM device_changes_acted")?;
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        Ok(self
            .device_changes_as_applied()?
            .into_iter()
            .filter(|change| !acted.contains(&(change.author.as_bytes().to_vec(), change.seq)))
            .collect())
    }

    /// What the admin's app did with `change`, if it acted on it.
    pub fn acted_on(&self, change: &RecordedChange) -> Result<Option<Acted>> {
        let acted: Option<String> = self
            .db
            .query_row(
                "SELECT acted FROM device_changes_acted WHERE author = ? AND seq = ?",
                params![change.author.as_bytes(), change.seq],
                |row| row.get(0),
            )
            .optional()?;
        acted.map(|acted| Acted::from_str(&acted)).transpose()
    }

    /// How many of `person`'s links the admin's app refused since `since_ms`.
    pub fn refusals_since(&self, person: &PublicKey, since_ms: i64) -> Result<usize> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM device_changes_acted AS acted
             JOIN device_changes AS change ON change.author = acted.author AND change.seq = acted.seq
             WHERE acted.acted = 'refused' AND change.person = ? AND acted.acted_ms >= ?",
            params![person.as_bytes(), since_ms],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count)?)
    }

    pub fn mark_acted(&self, change: &RecordedChange, acted: Acted, now_ms: i64) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO device_changes_acted (author, seq, acted, acted_ms) VALUES (?, ?, ?, ?)",
            params![change.author.as_bytes(), change.seq, acted.as_str(), now_ms],
        )?;
        Ok(())
    }

    /// As if a restore brought back a store from before this change was
    /// acted on.
    #[cfg(test)]
    pub fn forget_acted(&self, change: &RecordedChange) -> Result<()> {
        self.db.execute(
            "DELETE FROM device_changes_acted WHERE author = ? AND seq = ?",
            params![change.author.as_bytes(), change.seq],
        )?;
        Ok(())
    }

    pub fn put_owed_welcome(&self, owed: &OwedWelcome) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO link_deliveries
                (device, token, welcome, map_digest, epoch, devices_before, sealed_grant, until_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                owed.device.as_bytes(),
                owed.token.as_slice(),
                owed.welcome,
                owed.map_digest.as_slice(),
                owed.epoch,
                postcard::to_stdvec(&(&owed.devices_before, &owed.bound_frozen_before))?,
                owed.sealed_grant,
                owed.until_ms,
            ],
        )?;
        Ok(())
    }

    pub fn set_owed_grant(&self, device: &PublicKey, sealed_grant: &[u8]) -> Result<()> {
        self.db.execute(
            "UPDATE link_deliveries SET sealed_grant = ? WHERE device = ?",
            params![sealed_grant, device.as_bytes()],
        )?;
        Ok(())
    }

    pub fn owed_welcomes(&self) -> Result<Vec<OwedWelcome>> {
        let mut statement = self.db.prepare(
            "SELECT device, token, welcome, map_digest, epoch, devices_before, sealed_grant, until_ms
             FROM link_deliveries",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, u64>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, Option<Vec<u8>>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (device, token, welcome, map_digest, epoch, before, sealed_grant, until_ms) = row?;
            let (devices_before, bound_frozen_before) = postcard::from_bytes(&before)?;
            Ok(OwedWelcome {
                device: to_public_key(device)?,
                token: token.try_into().ok().context("malformed stored token")?,
                welcome,
                map_digest: map_digest.try_into().ok().context("malformed stored digest")?,
                epoch,
                devices_before,
                bound_frozen_before,
                sealed_grant,
                until_ms,
            })
        })
        .collect()
    }

    pub fn delete_owed_welcome(&self, device: &PublicKey) -> Result<()> {
        self.db.execute(
            "DELETE FROM link_deliveries WHERE device = ?",
            [device.as_bytes()],
        )?;
        Ok(())
    }

    pub fn put_owed_outcome(&self, token: &Token, device: &PublicKey, signed: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO pending_outcomes (token, device, outcome) VALUES (?, ?, ?)",
            params![token.as_slice(), device.as_bytes(), signed],
        )?;
        Ok(())
    }

    pub fn owed_outcomes(&self) -> Result<Vec<(Token, PublicKey, Vec<u8>)>> {
        let mut statement = self
            .db
            .prepare("SELECT token, device, outcome FROM pending_outcomes")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (token, device, outcome) = row?;
            Ok((
                token.try_into().ok().context("malformed stored token")?,
                to_public_key(device)?,
                outcome,
            ))
        })
        .collect()
    }

    pub fn delete_owed_outcome(&self, token: &Token, device: &PublicKey) -> Result<()> {
        self.db.execute(
            "DELETE FROM pending_outcomes WHERE token = ? AND device = ?",
            params![token.as_slice(), device.as_bytes()],
        )?;
        Ok(())
    }

    pub fn owe_key_only_commit(&self) -> Result<()> {
        self.set_meta(KEY_ONLY_COMMIT_OWED_META, &[1])
    }

    pub fn is_key_only_commit_owed(&self) -> Result<bool> {
        Ok(self.meta(KEY_ONLY_COMMIT_OWED_META)?.is_some())
    }

    /// Any commit moves the epoch, which is all a key-only commit is for.
    pub fn clear_key_only_commit_owed(&self) -> Result<()> {
        self.db
            .execute("DELETE FROM meta WHERE key = ?", [KEY_ONLY_COMMIT_OWED_META])?;
        Ok(())
    }
}
