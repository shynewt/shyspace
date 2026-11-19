//! Shared helpers: members on one in-memory bucket, driven without networking.

use std::{path::Path, sync::Arc, time::Duration};

use object_store::memory::InMemory;

use crate::{
    BucketConfig, Networking, Profile, Workspace, crypto, membership::invite::Invite,
    transport::bucket::Bucket,
};

pub fn test_bucket_config() -> BucketConfig {
    BucketConfig {
        endpoint: "http://unused".into(),
        region: "us-east-1".into(),
        bucket: "unused".into(),
        access_key_id: "unused".into(),
        secret_access_key: "unused".into(),
    }
}

pub async fn create_admin(dir: &Path, bucket: &Bucket) -> Workspace {
    Workspace::create_with(
        dir,
        "workspace-id".into(),
        "Acme",
        Profile {
            name: "Ada",
            is_bot: false,
        },
        test_bucket_config(),
        bucket.clone(),
        Networking::Offline,
    )
    .await
    .unwrap()
}

/// Bob asks to join while the admin's sync loop (driven here) accepts him.
pub async fn join(admin: &Workspace, dir: &Path, bucket: &Bucket, name: &str, is_bot: bool) -> Workspace {
    let invite = Invite::decode(&admin.invite_code().unwrap()).unwrap();
    let dir = dir.to_path_buf();
    let bucket_for_joiner = bucket.clone();
    let name = name.to_owned();
    let joining = tokio::spawn(async move {
        Workspace::join_with(
            &dir,
            invite,
            Profile { name: &name, is_bot },
            bucket_for_joiner,
            Networking::Offline,
        )
        .await
        .unwrap()
    });
    while !joining.is_finished() {
        admin.sync().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    joining.await.unwrap()
}

pub async fn sync_all(members: &[&Workspace]) {
    for _ in 0..2 {
        for member in members {
            member.sync().await;
            assert_eq!(member.status().error, None);
        }
    }
}

pub fn channel_id(workspace: &Workspace, name: &str) -> String {
    workspace
        .channels()
        .unwrap()
        .into_iter()
        .find(|channel| channel.name == name)
        .unwrap()
        .id
}

pub fn memory_bucket() -> Bucket {
    Bucket::new(Arc::new(InMemory::new()))
}

/// A workspace with an admin (Ada) and one member (Bob), in sync.
pub async fn admin_and_bob() -> (tempfile::TempDir, Bucket, Workspace, Workspace) {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;
    (root, bucket, admin, bob)
}

pub fn current_key(workspace: &Workspace) -> (u64, crypto::EpochKey) {
    let state = workspace.state();
    let epoch = state.mls.epoch().unwrap();
    (epoch, state.store.epoch_key(epoch).unwrap().unwrap())
}
