//! Links this device confirmed and vouched for, kept until the admin's app
//! adds or refuses the device, so the steps after the person's confirmation
//! (writing the link, handing over) resume after a restart. This device's
//! own state: it never leaves the device.

use anyhow::{Context, Result};
use rusqlite::params;

use super::Store;
use crate::membership::{
    device::DeviceCertificate,
    link::{LinkCode, LinkRequest, Token},
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
    /// When this device gives up on the link.
    pub until_ms: i64,
}

impl Store {
    pub fn put_pending_link(&self, link: &PendingLink) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO pending_links
                (token, code, request, certificate, answer_until_ms, handed_over, until_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                link.code.token.as_slice(),
                postcard::to_stdvec(&link.code)?,
                postcard::to_stdvec(&link.request)?,
                link.certificate.to_bytes()?,
                link.answer_until_ms,
                link.is_handed_over,
                link.until_ms,
            ],
        )?;
        Ok(())
    }

    pub fn pending_links(&self) -> Result<Vec<PendingLink>> {
        let mut statement = self.db.prepare(
            "SELECT code, request, certificate, answer_until_ms, handed_over, until_ms FROM pending_links",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (code, request, certificate, answer_until_ms, is_handed_over, until_ms) = row?;
            Ok(PendingLink {
                code: postcard::from_bytes(&code).context("reading a pending link's code")?,
                request: postcard::from_bytes(&request).context("reading a pending link's request")?,
                certificate: DeviceCertificate::from_bytes(&certificate)?,
                answer_until_ms,
                is_handed_over,
                until_ms,
            })
        })
        .collect()
    }

    pub fn delete_pending_link(&self, token: &Token) -> Result<()> {
        self.db
            .execute("DELETE FROM pending_links WHERE token = ?", [token.as_slice()])?;
        Ok(())
    }
}
