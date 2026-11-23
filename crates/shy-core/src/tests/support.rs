//! Shared helpers: members on one in-memory bucket, driven without networking.

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::Result;
use object_store::memory::InMemory;
use tokio::task::JoinHandle;

use crate::{
    BucketConfig, DataDir, JoinCancel, KeyStorage, Networking, Profile, Workspace, crypto,
    membership::invite::{Invite, JoinMailbox},
    ops::{Envelope, Op, OpBody, now_ms},
    pages::{self, CLIENT_ID_NONCE_BYTES, Edit},
    private::{
        self,
        crypto::Slot,
        ops::{PrivateBody, PrivateOp},
    },
    storage::StoredKey,
    transport::bucket::{Bucket, Connector},
    views::InviteState,
};

pub fn test_bucket_config() -> BucketConfig {
    BucketConfig {
        endpoint: "http://unused".into(),
        region: "us-east-1".into(),
        bucket: "unused".into(),
        access_key_id: "unused".into(),
        secret_access_key: "test-bucket-secret".into(),
    }
}

/// A member's data directory, with its key file beside it (never inside).
pub fn data_dir(dir: &Path) -> DataDir {
    DataDir::new(dir, KeyStorage::File(dir.with_extension("key")))
}

pub async fn create_admin(dir: &Path, bucket: &Bucket) -> Workspace {
    Workspace::create_with(
        &data_dir(dir),
        "workspace-id".into(),
        "Acme",
        Profile {
            name: "Ada",
            is_bot: false,
        },
        test_bucket_config(),
        connector(bucket),
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
    start_cancellable_join(invite, dir, bucket, name, is_bot, JoinCancel::new())
}

/// Like `start_join`, stopped while it waits by `cancel`.
pub fn start_cancellable_join(
    invite: Invite,
    dir: &Path,
    bucket: &Bucket,
    name: &str,
    is_bot: bool,
    cancel: JoinCancel,
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
            &data_dir(&dir),
            invite,
            profile,
            &mailbox,
            connector(&bucket),
            Networking::Offline,
            &cancel,
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

/// Connects every workspace to the same in-memory bucket, whatever the settings.
pub fn connector(bucket: &Bucket) -> Connector {
    let bucket = bucket.clone();
    Arc::new(move |_: &str, _: &BucketConfig| Ok(bucket.clone()))
}

/// The texts of the latest messages in `channel`, as `member` sees them.
pub fn texts_in(member: &Workspace, channel: &str) -> Vec<String> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect()
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

/// Seals ops as a member's next ones, in order, without them going through
/// their log.
pub struct Forger<'a> {
    author: &'a Workspace,
    pub next_seq: u64,
}

impl<'a> Forger<'a> {
    pub fn new(author: &'a Workspace) -> Self {
        let next_seq = author.state().store.next_seq(&author.me()).unwrap();
        Forger { author, next_seq }
    }

    pub fn seal(&mut self, op: Op) -> Envelope {
        self.seal_at(9_000 + self.next_seq, op)
    }

    /// Seals `op` at the next slot with a lamport clock of the forger's choosing.
    pub fn seal_at(&mut self, lamport: u64, op: Op) -> Envelope {
        let (epoch, key) = current_key(self.author);
        let seq = self.next_seq;
        self.next_seq += 1;
        let body = OpBody {
            lamport,
            timestamp_ms: now_ms().unwrap(),
            op,
        };
        crypto::seal(&self.author.inner.identity, &key, epoch, seq, &body).unwrap()
    }
}

impl Forger<'_> {
    /// Seals `op` into a private channel with `key`, bound to the next slot.
    pub fn seal_private(&mut self, key: &StoredKey, op: PrivateOp) -> Envelope {
        let body = PrivateBody {
            channel: key.channel.clone(),
            generation: key.generation,
            op,
        };
        let slot = Slot {
            workspace_id: &self.author.inner.config.workspace_id,
            author: &self.author.me(),
            seq: self.next_seq,
        };
        let payload =
            private::crypto::seal(key.labeled(), slot, &postcard::to_stdvec(&body).unwrap()).unwrap();
        self.seal(Op::Sealed { payload })
    }
}

/// `envelope` decrypted and encrypted again, with `extra` bytes after its
/// signed content, as anyone holding the epoch key could.
pub fn encrypted_again(envelope: &Envelope, epoch_key: &crypto::EpochKey, extra: usize) -> Envelope {
    use chacha20poly1305::{
        XChaCha20Poly1305, XNonce,
        aead::{Aead, Generate, KeyInit, Payload},
    };
    let header = crypto::header_bytes(&envelope.author, envelope.seq, envelope.epoch);
    let cipher = XChaCha20Poly1305::new(epoch_key.into());
    let mut plaintext = cipher
        .decrypt(
            &envelope.nonce.into(),
            Payload {
                msg: &envelope.ciphertext,
                aad: &header,
            },
        )
        .unwrap();
    plaintext.resize(plaintext.len() + extra, 0);
    let nonce = XNonce::generate();
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: &plaintext,
                aad: &header,
            },
        )
        .unwrap();
    Envelope {
        nonce: nonce.into(),
        ciphertext,
        ..envelope.clone()
    }
}

/// A nonce for an editor session; `page_update` uses this one.
pub const NONCE: [u8; CLIENT_ID_NONCE_BYTES] = [7; CLIENT_ID_NONCE_BYTES];

/// The bytes of `author`'s update making `edit` to `state`, written as the
/// Yjs client [`NONCE`] binds to them on `page`.
pub fn page_update_bytes(author: &Workspace, page: &str, state: Option<&[u8]>, edit: Edit) -> Vec<u8> {
    pages::edit(state, edit, pages::client_id(&author.me(), page, &NONCE)).unwrap()
}

/// `author`'s page update making `edit` to `state`, as their editor would
/// send it.
pub fn page_update(author: &Workspace, page: &str, state: Option<&[u8]>, edit: Edit) -> Op {
    Op::DocUpdate {
        page: page.to_owned(),
        nonce: NONCE,
        update: page_update_bytes(author, page, state, edit),
    }
}
