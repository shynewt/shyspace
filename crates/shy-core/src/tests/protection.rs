//! Telling whether the bucket keeps deleted or overwritten data. A wrong
//! "safe" would hide the warning, so each way a check could be fooled has a
//! test here.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use async_trait::async_trait;
use futures_lite::{StreamExt, stream};
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore, ObjectStoreExt,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory, path::Path,
};

use super::support::*;
use crate::{Networking, Workspace, transport::bucket::Bucket, views::StorageProtection};

/// What a provider does with versions.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Versions {
    None,
    /// Sends the version id S3 uses when versioning was never on.
    Null,
    /// Sends a version id but serves the current object for any version.
    IgnoredOnRead,
    Kept,
}

/// A bucket that behaves like one provider setup or another.
#[derive(Debug)]
struct FakeStore {
    current: InMemory,
    old_versions: InMemory,
    versions: Versions,
    /// Deletes and overwrites under `ops/` are refused, like an R2 lock.
    is_ops_locked: bool,
    /// The next delete fails, like a flaky network.
    fails_next_delete: AtomicBool,
    is_down: AtomicBool,
    writes: AtomicU32,
}

impl FakeStore {
    fn new(versions: Versions, is_ops_locked: bool) -> Self {
        Self {
            current: InMemory::new(),
            old_versions: InMemory::new(),
            versions,
            is_ops_locked,
            fails_next_delete: AtomicBool::new(false),
            is_down: AtomicBool::new(false),
            writes: AtomicU32::new(0),
        }
    }

    fn is_locked(&self, location: &Path) -> bool {
        self.is_ops_locked && location.as_ref().starts_with("ops/")
    }

    fn reachable(&self) -> object_store::Result<()> {
        if self.is_down.load(Ordering::SeqCst) {
            return Err(object_store::Error::Generic {
                store: "fake",
                source: "connection refused".into(),
            });
        }
        Ok(())
    }

    fn refused(location: &Path) -> object_store::Error {
        object_store::Error::PermissionDenied {
            path: location.to_string(),
            source: "refused".into(),
        }
    }
}

impl fmt::Display for FakeStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fake test store")
    }
}

fn version_path(location: &Path, version: &str) -> Path {
    Path::from(format!("{location}/{version}"))
}

#[async_trait]
impl ObjectStore for FakeStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.reachable()?;
        if self.is_locked(location) && self.current.head(location).await.is_ok() {
            return Err(Self::refused(location));
        }
        let count = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
        let version = count.to_string();
        self.old_versions
            .put_opts(
                &version_path(location, &version),
                payload.clone(),
                PutOptions::default(),
            )
            .await?;
        let mut result = self.current.put_opts(location, payload, opts).await?;
        result.version = match self.versions {
            Versions::None => None,
            Versions::Null => Some("null".to_owned()),
            Versions::IgnoredOnRead | Versions::Kept => Some(version),
        };
        Ok(result)
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.current.put_multipart_opts(location, opts).await
    }

    async fn get_opts(&self, location: &Path, options: GetOptions) -> object_store::Result<GetResult> {
        self.reachable()?;
        let version = options.version.clone();
        let options = GetOptions {
            version: None,
            ..options
        };
        match (version, self.versions) {
            (Some(version), Versions::Kept) => {
                self.old_versions
                    .get_opts(&version_path(location, &version), options)
                    .await
            }
            _ => self.current.get_opts(location, options).await,
        }
    }

    fn delete_stream(
        &self,
        locations: stream::Boxed<object_store::Result<Path>>,
    ) -> stream::Boxed<object_store::Result<Path>> {
        let current = self.current.clone();
        let is_ops_locked = self.is_ops_locked;
        let fails_now = self.fails_next_delete.swap(false, Ordering::SeqCst);
        Box::pin(locations.then(move |location| {
            let current = current.clone();
            async move {
                let path = location?;
                if fails_now {
                    return Err(object_store::Error::Generic {
                        store: "fake",
                        source: "timed out".into(),
                    });
                }
                if is_ops_locked && path.as_ref().starts_with("ops/") {
                    return Err(Self::refused(&path));
                }
                current.delete(&path).await?;
                Ok(path)
            }
        }))
    }

    fn list(&self, prefix: Option<&Path>) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        self.current.list(prefix)
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.current.list_with_delimiter(prefix).await
    }

    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> object_store::Result<()> {
        self.current.copy_opts(from, to, options).await
    }
}

async fn admin_on(store: Arc<FakeStore>) -> (tempfile::TempDir, Bucket, Workspace) {
    let root = tempfile::tempdir().unwrap();
    let bucket = Bucket::without_signing(store);
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    (root, bucket, admin)
}

async fn checked(store: FakeStore) -> StorageProtection {
    let (_root, _bucket, admin) = admin_on(Arc::new(store)).await;
    admin.check_storage_protection().await.unwrap().state
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lock_on_the_ops_counts_as_locked_and_keeps_counting() {
    let (_root, _bucket, admin) = admin_on(Arc::new(FakeStore::new(Versions::None, true))).await;
    for _ in 0..2 {
        // The second check finds the last check's probe, which it can't overwrite.
        assert_eq!(
            admin.check_storage_protection().await.unwrap().state,
            StorageProtection::Locked
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn kept_versions_count_as_versioned_not_locked() {
    assert_eq!(
        checked(FakeStore::new(Versions::Kept, false)).await,
        StorageProtection::Versioned
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn version_ids_that_keep_nothing_count_as_unprotected() {
    for versions in [Versions::None, Versions::Null, Versions::IgnoredOnRead] {
        assert_eq!(
            checked(FakeStore::new(versions, false)).await,
            StorageProtection::Unprotected,
            "{versions:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn one_failed_delete_is_not_mistaken_for_a_lock() {
    let store = Arc::new(FakeStore::new(Versions::None, false));
    let (_root, _bucket, admin) = admin_on(store.clone()).await;
    store.fails_next_delete.store(true, Ordering::SeqCst);
    assert_eq!(
        admin.check_storage_protection().await.unwrap().state,
        StorageProtection::Unprotected
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_check_is_kept_as_unknown_and_retried_later() {
    let store = Arc::new(FakeStore::new(Versions::Kept, true));
    let (root, bucket, admin) = admin_on(store.clone()).await;
    assert_eq!(
        admin.check_storage_protection().await.unwrap().state,
        StorageProtection::Locked
    );

    store.is_down.store(true, Ordering::SeqCst);
    assert!(admin.check_storage_protection().await.is_err());
    let failed = admin.storage_protection().unwrap();
    assert_eq!(
        failed.state,
        StorageProtection::Unknown,
        "no stale result lingers"
    );
    assert!(failed.checked_ms.is_some());

    // Syncing soon after doesn't check again.
    store.is_down.store(false, Ordering::SeqCst);
    admin.sync().await;
    assert_eq!(admin.storage_protection().unwrap(), failed);
    drop(admin);

    let reopened = Workspace::open_with(
        &data_dir(&root.path().join("ada")),
        connector(&bucket),
        Networking::Offline,
    )
    .await
    .unwrap();
    assert_eq!(
        reopened.storage_protection().unwrap(),
        failed,
        "kept across restarts"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plain_bucket_is_checked_once_a_day_while_syncing() {
    let (_root, _bucket, admin) = admin_on(Arc::new(FakeStore::new(Versions::None, false))).await;
    assert_eq!(
        admin.storage_protection().unwrap().state,
        StorageProtection::Unknown
    );
    admin.sync().await;
    let first = admin.storage_protection().unwrap();
    assert_eq!(first.state, StorageProtection::Unprotected);
    admin.sync().await;
    assert_eq!(admin.storage_protection().unwrap(), first);
}
