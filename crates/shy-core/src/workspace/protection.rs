//! Whether the bucket keeps deleted or overwritten data. Checked when due
//! during sync (daily, or hourly after a failed check), on request, and after
//! the storage keys change; the last result is kept so the app can warn
//! without asking the bucket each time.

use anyhow::{Context, Result};

use super::Workspace;
use crate::{
    ops::now_ms,
    views::{Change, ProtectionCheck, StorageProtection},
};

const PROTECTION_META: &str = "storage_protection";
const CHECK_EVERY_MS: i64 = 24 * 60 * 60 * 1000;
const RETRY_FAILED_CHECK_AFTER_MS: i64 = 60 * 60 * 1000;

impl Workspace {
    /// The last check's result, without asking the bucket.
    pub fn storage_protection(&self) -> Result<ProtectionCheck> {
        match self.state().store.meta(PROTECTION_META)? {
            None => Ok(ProtectionCheck {
                state: StorageProtection::Unknown,
                checked_ms: None,
            }),
            Some(bytes) => postcard::from_bytes(&bytes).context("reading the last storage check"),
        }
    }

    /// Checks the bucket now and keeps the result. A failed check is kept as
    /// unknown, so a stale result doesn't linger, and the error returned.
    pub async fn check_storage_protection(&self) -> Result<ProtectionCheck> {
        let checked = self.bucket().check_protection(&self.me()).await;
        let check = ProtectionCheck {
            state: *checked.as_ref().unwrap_or(&StorageProtection::Unknown),
            checked_ms: Some(now_ms()?),
        };
        self.state()
            .store
            .set_meta(PROTECTION_META, &postcard::to_stdvec(&check)?)?;
        self.emit(Change::StorageProtection);
        checked.map(|_| check)
    }

    pub(crate) fn forget_storage_protection(&self) -> Result<()> {
        self.state().store.delete_meta(PROTECTION_META)?;
        self.emit(Change::StorageProtection);
        Ok(())
    }

    pub(crate) async fn check_storage_protection_when_due(&self) -> Result<()> {
        let last = self.storage_protection()?;
        let wait_ms = match last.state {
            StorageProtection::Unknown => RETRY_FAILED_CHECK_AFTER_MS,
            _ => CHECK_EVERY_MS,
        };
        let now = now_ms()?;
        if last
            .checked_ms
            .is_some_and(|checked_ms| now - checked_ms < wait_ms)
        {
            return Ok(());
        }
        self.check_storage_protection().await.map(drop)
    }
}
