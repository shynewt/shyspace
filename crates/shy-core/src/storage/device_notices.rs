//! Notices about the person's own devices, for their own screens only: a
//! device was linked, or removed. This device's own state: never shared,
//! and left out of the admin's snapshot.

use anyhow::{Result, bail};
use iroh::PublicKey;
use rusqlite::params;

use super::{Store, to_public_key};

/// How long a notice is shown.
pub const DEVICE_NOTICE_LIFETIME_MS: i64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceNoticeKind {
    Linked,
    Removed,
}

impl DeviceNoticeKind {
    fn as_number(self) -> i64 {
        match self {
            Self::Linked => 0,
            Self::Removed => 1,
        }
    }

    fn from_number(number: i64) -> Result<Self> {
        match number {
            0 => Ok(Self::Linked),
            1 => Ok(Self::Removed),
            _ => bail!("an unknown device notice"),
        }
    }
}

/// A kept notice: which device, what happened, when, and whether seen.
pub struct KeptDeviceNotice {
    pub device: PublicKey,
    pub kind: DeviceNoticeKind,
    pub at_ms: i64,
    pub is_seen: bool,
}

impl Store {
    /// Notes what happened to one of the person's devices. Noting it again
    /// (a rebuild applying the same op) keeps the first, and its seen mark.
    pub fn note_device(&self, device: &PublicKey, kind: DeviceNoticeKind, at_ms: i64) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO device_notices (device, kind, at_ms, seen) VALUES (?, ?, ?, 0)",
            params![device.as_bytes(), kind.as_number(), at_ms],
        )?;
        Ok(())
    }

    /// Notices from `since_ms` on, newest first.
    pub fn device_notices(&self, since_ms: i64) -> Result<Vec<KeptDeviceNotice>> {
        let mut statement = self.db.prepare(
            "SELECT device, kind, at_ms, seen FROM device_notices WHERE at_ms >= ? ORDER BY at_ms DESC, kind DESC",
        )?;
        let rows = statement.query_map([since_ms], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (device, kind, at_ms, is_seen) = row?;
            Ok(KeptDeviceNotice {
                device: to_public_key(device)?,
                kind: DeviceNoticeKind::from_number(kind)?,
                at_ms,
                is_seen,
            })
        })
        .collect()
    }

    pub fn mark_device_notices_seen(&self) -> Result<()> {
        self.db.execute("UPDATE device_notices SET seen = 1", [])?;
        Ok(())
    }
}
