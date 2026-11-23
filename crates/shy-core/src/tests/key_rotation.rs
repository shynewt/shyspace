//! Changing the bucket key after a member is removed (#135): the prompt,
//! who has moved, and proving the old key dead.

use std::{
    collections::HashSet,
    fmt,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use futures_lite::stream;
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory, path::Path,
};

use super::support::*;
use crate::{
    BucketConfig, Workspace,
    transport::bucket::{Bucket, Connector},
    views::{MemberMove, OldKeyCheck, OldKeyState, StorageProvider, member_id},
    workspace::StorageUpdate,
};

pub const FIRST_KEY: &str = "unused";

#[derive(Debug, Default)]
struct KeyRules {
    refused: HashSet<String>,
    unreachable: HashSet<String>,
}

/// A provider in front of one shared bucket: it refuses keys that were
/// deleted (403, as S3 does) and can't be reached with others.
#[derive(Clone)]
pub struct Provider {
    store: Arc<InMemory>,
    rules: Arc<Mutex<KeyRules>>,
}

impl Provider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(InMemory::new()),
            rules: Arc::default(),
        }
    }

    /// The bucket as invite links reach it, whatever the keys.
    pub fn bucket(&self) -> Bucket {
        Bucket::without_signing(self.store.clone())
    }

    pub fn connector(&self) -> Connector {
        let provider = self.clone();
        Arc::new(move |_: &str, config: &BucketConfig| {
            Ok(Bucket::without_signing(Arc::new(KeyedStore {
                inner: provider.store.clone(),
                key: config.access_key_id.clone(),
                rules: provider.rules.clone(),
            })))
        })
    }

    pub fn delete_key(&self, key: &str) {
        self.rules.lock().unwrap().refused.insert(key.to_owned());
    }

    pub fn cut_off(&self, key: &str) {
        self.rules.lock().unwrap().unreachable.insert(key.to_owned());
    }

    pub fn reconnect(&self, key: &str) {
        self.rules.lock().unwrap().unreachable.remove(key);
    }
}

#[derive(Debug)]
struct KeyedStore {
    inner: Arc<InMemory>,
    key: String,
    rules: Arc<Mutex<KeyRules>>,
}

impl fmt::Display for KeyedStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("keyed test store")
    }
}

impl KeyedStore {
    fn refusal(&self, path: &str) -> Option<object_store::Error> {
        let rules = self.rules.lock().unwrap();
        if rules.refused.contains(&self.key) {
            return Some(object_store::Error::PermissionDenied {
                path: path.to_owned(),
                source: "InvalidAccessKeyId".into(),
            });
        }
        rules
            .unreachable
            .contains(&self.key)
            .then(|| object_store::Error::Generic {
                store: "keyed",
                source: "connection refused".into(),
            })
    }

    fn refused_listing(
        &self,
        prefix: Option<&Path>,
    ) -> Option<stream::Boxed<object_store::Result<ObjectMeta>>> {
        let path = prefix.map(Path::to_string).unwrap_or_default();
        self.refusal(&path)
            .map(|error| -> stream::Boxed<object_store::Result<ObjectMeta>> {
                Box::pin(stream::once(Err(error)))
            })
    }
}

#[async_trait]
impl ObjectStore for KeyedStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        if let Some(error) = self.refusal(location.as_ref()) {
            return Err(error);
        }
        self.inner.put_opts(location, payload, opts).await
    }
    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        if let Some(error) = self.refusal(location.as_ref()) {
            return Err(error);
        }
        self.inner.put_multipart_opts(location, opts).await
    }
    async fn get_opts(&self, location: &Path, options: GetOptions) -> object_store::Result<GetResult> {
        if let Some(error) = self.refusal(location.as_ref()) {
            return Err(error);
        }
        self.inner.get_opts(location, options).await
    }
    fn delete_stream(
        &self,
        locations: stream::Boxed<object_store::Result<Path>>,
    ) -> stream::Boxed<object_store::Result<Path>> {
        if let Some(error) = self.refusal("") {
            return Box::pin(stream::once(Err(error)));
        }
        self.inner.delete_stream(locations)
    }
    fn list(&self, prefix: Option<&Path>) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        self.refused_listing(prefix)
            .unwrap_or_else(|| self.inner.list(prefix))
    }
    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        self.refused_listing(prefix)
            .unwrap_or_else(|| self.inner.list_with_offset(prefix, offset))
    }
    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        if let Some(error) = self.refusal(&prefix.map(Path::to_string).unwrap_or_default()) {
            return Err(error);
        }
        self.inner.list_with_delimiter(prefix).await
    }
    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> object_store::Result<()> {
        if let Some(error) = self.refusal(to.as_ref()) {
            return Err(error);
        }
        self.inner.copy_opts(from, to, options).await
    }
}

/// An admin, Bob and Carol on `provider`, in sync.
pub async fn admin_bob_carol(provider: &Provider) -> (tempfile::TempDir, Workspace, Workspace, Workspace) {
    let root = tempfile::tempdir().unwrap();
    let admin = create_admin_connected(&root.path().join("ada"), provider.connector()).await;
    let bucket = provider.bucket();
    let bob = join_connected(
        &admin,
        &root.path().join("bob"),
        &bucket,
        provider.connector(),
        "Bob",
    )
    .await;
    let carol = join_connected(
        &admin,
        &root.path().join("carol"),
        &bucket,
        provider.connector(),
        "Carol",
    )
    .await;
    sync_all(&[&admin, &bob, &carol]).await;
    (root, admin, bob, carol)
}

pub async fn change_key(admin: &Workspace, key: &str) {
    admin
        .update_storage(StorageUpdate {
            region: "us-east-1",
            access_key_id: key,
            secret_access_key: &format!("{key}-secret"),
        })
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_member_asks_the_admin_to_change_the_bucket_key() {
    let provider = Provider::new();
    let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
    assert!(admin.storage_rotation().await.unwrap().is_none());

    admin.remove_member(&carol.my_id()).await.unwrap();

    let rotation = admin
        .storage_rotation()
        .await
        .unwrap()
        .expect("a rotation is due");
    assert_eq!(rotation.provider, StorageProvider::Other);
    assert!(!rotation.has_new_key);
    assert_eq!(rotation.old_key, OldKeyState::NotChecked);
    assert_eq!(
        rotation.members,
        [MemberMove {
            member: member_id(&bob.me()),
            has_moved: false
        }]
    );
    assert!(
        bob.storage_rotation().await.unwrap().is_none(),
        "only the admin is asked"
    );
    assert!(
        admin.check_old_key_deleted().await.is_err(),
        "there is no old key until a new one is saved"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn members_who_picked_up_the_new_key_show_as_moved() {
    let provider = Provider::new();
    let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
    admin.remove_member(&carol.my_id()).await.unwrap();

    change_key(&admin, "new-key").await;
    let rotation = admin.storage_rotation().await.unwrap().unwrap();
    assert!(rotation.has_new_key);
    assert!(!rotation.members[0].has_moved);

    admin.sync().await;
    bob.sync().await;
    bob.check().await;
    let rotation = admin.storage_rotation().await.unwrap().unwrap();
    assert!(rotation.members[0].has_moved);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_old_key_counts_as_deleted_only_when_the_provider_refuses_it() {
    let provider = Provider::new();
    let (_root, admin, _bob, carol) = admin_bob_carol(&provider).await;
    admin.remove_member(&carol.my_id()).await.unwrap();
    change_key(&admin, "new-key").await;

    assert_eq!(
        admin.check_old_key_deleted().await.unwrap(),
        OldKeyCheck::StillWorks
    );
    assert_eq!(
        admin.storage_rotation().await.unwrap().unwrap().old_key,
        OldKeyState::StillWorks
    );

    provider.cut_off(FIRST_KEY);
    assert_eq!(
        admin.check_old_key_deleted().await.unwrap(),
        OldKeyCheck::Unknown,
        "a network error isn't a deleted key"
    );

    provider.reconnect(FIRST_KEY);
    provider.delete_key(FIRST_KEY);
    provider.cut_off("new-key");
    assert_eq!(
        admin.check_old_key_deleted().await.unwrap(),
        OldKeyCheck::Unknown,
        "the new key failing too means it can't tell"
    );

    provider.reconnect("new-key");
    assert_eq!(admin.check_old_key_deleted().await.unwrap(), OldKeyCheck::Dead);
    assert!(admin.storage_rotation().await.unwrap().is_none());
    assert_eq!(admin.kept_old_keys().unwrap(), 0, "the old secret is erased");
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_rotation_erases_the_old_key() {
    let provider = Provider::new();
    let (_root, admin, _bob, carol) = admin_bob_carol(&provider).await;
    admin.remove_member(&carol.my_id()).await.unwrap();
    change_key(&admin, "new-key").await;
    assert_eq!(admin.kept_old_keys().unwrap(), 1);

    admin.cancel_storage_rotation().unwrap();

    assert!(admin.storage_rotation().await.unwrap().is_none());
    assert_eq!(admin.kept_old_keys().unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_check_that_couldnt_tell_is_tried_again_in_the_background() {
    let provider = Provider::new();
    let (_root, admin, _bob, carol) = admin_bob_carol(&provider).await;
    admin.remove_member(&carol.my_id()).await.unwrap();
    change_key(&admin, "new-key").await;
    provider.cut_off(FIRST_KEY);
    assert_eq!(admin.check_old_key_deleted().await.unwrap(), OldKeyCheck::Unknown);

    provider.reconnect(FIRST_KEY);
    provider.delete_key(FIRST_KEY);
    admin.make_slow_duties_due();
    admin.check().await;

    assert!(admin.storage_rotation().await.unwrap().is_none());
    assert_eq!(admin.kept_old_keys().unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn every_key_replaced_during_a_rotation_must_be_dead_before_it_ends() {
    let provider = Provider::new();
    let (_root, admin, _bob, carol) = admin_bob_carol(&provider).await;
    admin.remove_member(&carol.my_id()).await.unwrap();
    change_key(&admin, "second-key").await;
    change_key(&admin, "third-key").await;
    assert_eq!(admin.kept_old_keys().unwrap(), 2);

    provider.delete_key(FIRST_KEY);
    assert_eq!(
        admin.check_old_key_deleted().await.unwrap(),
        OldKeyCheck::StillWorks
    );
    assert_eq!(
        admin.kept_old_keys().unwrap(),
        1,
        "the dead one is erased at once"
    );

    provider.delete_key("second-key");
    assert_eq!(admin.check_old_key_deleted().await.unwrap(), OldKeyCheck::Dead);
    assert_eq!(admin.kept_old_keys().unwrap(), 0);
}
