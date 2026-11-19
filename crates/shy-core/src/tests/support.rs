//! Shared helpers: members on one in-memory bucket, driven without networking.

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::Result;
use object_store::memory::InMemory;
use tokio::task::JoinHandle;

use crate::{
    BucketConfig, Networking, Profile, Workspace, crypto,
    membership::invite::{Invite, JoinMailbox},
    transport::bucket::Bucket,
    views::InviteState,
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

/// An invitee's mailbox that talks to the in-memory bucket directly, where
/// production uses presigned links.
pub struct BucketMailbox {
    pub bucket: Bucket,
    pub token: String,
}

impl JoinMailbox for BucketMailbox {
    async fn post_request(&self, request: Vec<u8>) -> Result<()> {
        self.bucket.put_invite_request(&self.token, request).await
    }

    async fn fetch_answer(&self) -> Result<Option<Vec<u8>>> {
        Ok(self
            .bucket
            .invite_answer(&self.token)
            .await?
            .map(|bytes| bytes.to_vec()))
    }
}

/// Starts joining with `invite` in the background.
pub fn start_join(
    invite: Invite,
    dir: &Path,
    bucket: &Bucket,
    name: &str,
    is_bot: bool,
) -> JoinHandle<Result<Workspace>> {
    let dir = dir.to_path_buf();
    let bucket = bucket.clone();
    let name = name.to_owned();
    tokio::spawn(async move {
        let mailbox = BucketMailbox {
            bucket: bucket.clone(),
            token: invite.token.clone(),
        };
        let profile = Profile { name: &name, is_bot };
        Workspace::join_with(
            &dir,
            invite,
            profile,
            &mailbox,
            |_| Ok(bucket),
            Networking::Offline,
        )
        .await
    })
}

/// Syncs the admin until the invite's request arrives.
pub async fn wait_for_request(admin: &Workspace, token: &str) {
    for _ in 0..200 {
        admin.sync().await;
        let invite = admin
            .invites()
            .unwrap()
            .into_iter()
            .find(|invite| invite.token == token)
            .unwrap();
        if invite.state == InviteState::Requested {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the join request never arrived");
}

/// A new member joins with a fresh invite, which the admin approves.
pub async fn join(admin: &Workspace, dir: &Path, bucket: &Bucket, name: &str, is_bot: bool) -> Workspace {
    let invite = admin.create_test_invite(name, Duration::from_secs(600)).unwrap();
    let token = invite.token.clone();
    let joining = start_join(invite, dir, bucket, name, is_bot);
    wait_for_request(admin, &token).await;
    admin.approve_invite(&token).await.unwrap();
    joining.await.unwrap().unwrap()
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
    Bucket::without_signing(Arc::new(InMemory::new()))
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
