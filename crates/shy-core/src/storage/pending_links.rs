//! Links this device confirmed and vouched for, kept until the admin's app
//! adds or refuses the device, so the steps after the person's confirmation
//! (writing the link, handing over) resume after a restart. This device's
//! own state: it never leaves the device.

use anyhow::{Context, Result};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::membership::{
    device::DeviceCertificate,
    link::{LinkCode, LinkRequest, Platform, Token},
};

/// A link this device confirmed.
#[derive(Debug, Clone)]
pub struct PendingLink {
    pub code: LinkCode,
    pub request: LinkRequest,
    pub certificate: DeviceCertificate,
    /// When the new device's link to read the answer slot stops working.
    pub answer_until_ms: i64,
    pub is_handed_over: bool,
    /// The admin's app refused it; kept until `until_ms` so the new device
    /// can still read the outcome.
    pub is_refused: bool,
    /// The request and answer slots are gone from the bucket.
    pub are_slots_deleted: bool,
    /// When this device gives up on the link.
    pub until_ms: i64,
}

impl Store {
    pub fn put_pending_link(&self, link: &PendingLink) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO pending_links
                (token, code, request, certificate, answer_until_ms, handed_over, refused, slots_deleted, until_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                link.code.token.as_slice(),
                postcard::to_stdvec(&link.code)?,
                postcard::to_stdvec(&link.request)?,
                link.certificate.to_bytes()?,
                link.answer_until_ms,
                link.is_handed_over,
                link.is_refused,
                link.are_slots_deleted,
                link.until_ms,
            ],
        )?;
        Ok(())
    }

    pub fn pending_links(&self) -> Result<Vec<PendingLink>> {
        let mut statement = self.db.prepare(
            "SELECT code, request, certificate, answer_until_ms, handed_over, refused, slots_deleted, until_ms
             FROM pending_links",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, bool>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (
                code,
                request,
                certificate,
                answer_until_ms,
                is_handed_over,
                is_refused,
                are_slots_deleted,
                until_ms,
            ) = row?;
            Ok(PendingLink {
                code: postcard::from_bytes(&code).context("reading a pending link's code")?,
                request: postcard::from_bytes(&request).context("reading a pending link's request")?,
                certificate: DeviceCertificate::from_bytes(&certificate)?,
                answer_until_ms,
                is_handed_over,
                is_refused,
                are_slots_deleted,
                until_ms,
            })
        })
        .collect()
    }

    /// What a device this one linked runs on, for the person's own screens.
    pub fn put_device_platform(&self, device: &PublicKey, platform: Platform) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO device_platforms (device, platform) VALUES (?, ?)",
            params![device.as_bytes(), postcard::to_stdvec(&platform)?],
        )?;
        Ok(())
    }

    pub fn device_platform(&self, device: &PublicKey) -> Result<Option<Platform>> {
        let bytes: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT platform FROM device_platforms WHERE device = ?",
                [device.as_bytes()],
                |row| row.get(0),
            )
            .optional()?;
        bytes
            .map(|bytes| postcard::from_bytes(&bytes).context("reading a device's platform"))
            .transpose()
    }

    pub fn delete_pending_link(&self, token: &Token) -> Result<()> {
        self.db
            .execute("DELETE FROM pending_links WHERE token = ?", [token.as_slice()])?;
        Ok(())
    }
}
