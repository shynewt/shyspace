//! The bucket settings. The admin can change the region and keys (to fix a
//! mistake or rotate credentials); members pick the change up as an op and
//! reconnect. Moving to another endpoint or bucket would leave the data
//! behind, so those stay fixed.

use anyhow::{Context, Result, ensure};
use tracing::warn;

use super::Workspace;
use crate::{
    config,
    ops::Op,
    transport::bucket::BucketConfig,
    views::{Change, StorageSettings},
};

/// What the admin can change.
pub struct StorageUpdate<'a> {
    pub region: &'a str,
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
}

impl Workspace {
    pub(crate) fn bucket_config(&self) -> Result<BucketConfig> {
        config::current_bucket(&self.state().store)
    }

    pub fn storage_settings(&self) -> Result<StorageSettings> {
        let bucket = self.bucket_config()?;
        Ok(StorageSettings {
            workspace_id: self.inner.config.workspace_id.clone(),
            endpoint: bucket.endpoint,
            bucket: bucket.bucket,
            region: bucket.region,
            access_key_id: bucket.access_key_id,
        })
    }

    /// Checks the new settings against the bucket, then sends them to every
    /// current member. Keep the old key active until members have synced.
    pub async fn update_storage(&self, update: StorageUpdate<'_>) -> Result<()> {
        ensure!(
            self.is_admin(),
            "only the workspace admin can change storage settings"
        );
        let current = self.bucket_config()?;
        let next = BucketConfig {
            region: update.region.trim().to_owned(),
            access_key_id: update.access_key_id.trim().to_owned(),
            secret_access_key: update.secret_access_key.trim().to_owned(),
            ..current
        };
        ensure!(
            !next.region.is_empty() && !next.access_key_id.is_empty() && !next.secret_access_key.is_empty(),
            "region, access key and secret key are all needed"
        );
        let bucket = (self.inner.connect)(&self.inner.config.workspace_id, &next)?;
        bucket
            .check_access()
            .await
            .context("checking the new storage settings")?;
        self.write(Op::StorageSettings { bucket: next })?;
        Workspace::mark_recovery_snapshot_stale(&self.state().store)?;
        self.refresh_recovery_snapshot().await
    }

    /// Reconnects with the settings now stored, after they changed.
    pub(crate) fn reconnect_bucket(&self) {
        let reconnected = self
            .bucket_config()
            .and_then(|config| (self.inner.connect)(&self.inner.config.workspace_id, &config));
        match reconnected {
            Ok(bucket) => {
                *self
                    .inner
                    .bucket
                    .write()
                    .expect("bucket lock poisoned by an earlier panic") = bucket;
                // New keys can have other rights; the next sync checks again.
                if let Err(error) = self.forget_storage_protection() {
                    warn!("can't forget the last storage check: {error:#}");
                }
                self.inner.wake.notify_one();
            }
            Err(error) => warn!("can't use the new storage settings: {error:#}"),
        }
    }

    /// Reconnects first, so listeners already see the new bucket in use.
    pub(crate) fn on_storage_changed(&self, change: &Change) {
        if *change == Change::Storage {
            self.reconnect_bucket();
        }
    }
}
