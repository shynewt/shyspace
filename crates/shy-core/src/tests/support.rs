//! Shared helpers: members on one in-memory bucket, driven without networking.

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::Result;
use object_store::memory::InMemory;
use tokio::task::JoinHandle;

use crate::{
    BucketConfig, DataDir, DeviceEnrollment, DeviceLink, DeviceRequest, JoinCancel, KeyStorage, Networking,
    Profile, Workspace, WorkspaceOptions, crypto,
    membership::{
        commit::{self, FrozenAuthor},
        device::{self, DeviceCertificate},
        invite::{Invite, JoinMailbox},
        mls::{Mls, binding_message},
    },
    ops::{Envelope, Op, OpBody, now_ms},
    pages::{self, CLIENT_ID_NONCE_BYTES, Edit},
    private::{
        self,
        crypto::Slot,
        ops::{PrivateBody, PrivateOp},
    },
    storage::{StoredKey, TEST_DB_KEY},
    transport::bucket::{Bucket, Connector},
    views::InviteState,
};

/// The id tests' workspaces get, shaped like a real one (see `links`).
pub const TEST_WORKSPACE_ID: &str = "0123456789abcdef";

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
    create_admin_with(dir, bucket, Networking::Offline.into()).await
}

pub async fn create_admin_with(dir: &Path, bucket: &Bucket, options: WorkspaceOptions) -> Workspace {
    create_admin_using(dir, connector(bucket), options).await
}

/// Like `create_admin`, reaching the bucket through `connect`.
pub async fn create_admin_connected(dir: &Path, connect: Connector) -> Workspace {
    create_admin_using(dir, connect, Networking::Offline.into()).await
}

async fn create_admin_using(dir: &Path, connect: Connector, options: WorkspaceOptions) -> Workspace {
    Workspace::create_with(
        &data_dir(dir),
        TEST_WORKSPACE_ID.into(),
        "Acme",
        Profile {
            name: "Ada",
            is_bot: false,
        },
        test_bucket_config(),
        connect,
        options,
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

/// A new device's mailbox for a link code, talking to the in-memory bucket
/// directly where production uses the code's presigned links.
pub struct BucketLinkMailbox {
    pub bucket: Bucket,
    pub token: crate::membership::link::Token,
}

impl JoinMailbox for BucketLinkMailbox {
    async fn post_request(&self, request: Vec<u8>) -> Result<()> {
        self.bucket.put_link_slot(&self.token, "request", request).await
    }

    async fn fetch_answer(&self) -> Result<Option<Vec<u8>>> {
        Ok(self
            .bucket
            .link_slot(&self.token, "answer")
            .await?
            .map(|bytes| bytes.to_vec()))
    }
}

/// A link mailbox on the in-memory bucket whose first posts and answer
/// fetches fail, as storage out of reach does.
pub struct FlakyLinkMailbox {
    inner: BucketLinkMailbox,
    post_failures_left: std::sync::atomic::AtomicUsize,
    fetch_failures_left: std::sync::atomic::AtomicUsize,
}

impl FlakyLinkMailbox {
    pub fn new(
        bucket: Bucket,
        token: crate::membership::link::Token,
        post_failures: usize,
        fetch_failures: usize,
    ) -> Self {
        Self {
            inner: BucketLinkMailbox { bucket, token },
            post_failures_left: std::sync::atomic::AtomicUsize::new(post_failures),
            fetch_failures_left: std::sync::atomic::AtomicUsize::new(fetch_failures),
        }
    }
}

/// Spends one of `left`, if any: whether this call fails.
fn spends_failure(left: &std::sync::atomic::AtomicUsize) -> bool {
    left.try_update(
        std::sync::atomic::Ordering::SeqCst,
        std::sync::atomic::Ordering::SeqCst,
        |left| left.checked_sub(1),
    )
    .is_ok()
}

impl JoinMailbox for FlakyLinkMailbox {
    async fn post_request(&self, request: Vec<u8>) -> Result<()> {
        anyhow::ensure!(
            !spends_failure(&self.post_failures_left),
            "can't reach the workspace's storage"
        );
        self.inner.post_request(request).await
    }

    async fn fetch_answer(&self) -> Result<Option<Vec<u8>>> {
        anyhow::ensure!(
            !spends_failure(&self.fetch_failures_left),
            "can't reach the workspace's storage"
        );
        self.inner.fetch_answer().await
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
    start_join_with(
        invite,
        dir,
        bucket,
        name,
        is_bot,
        cancel,
        Networking::Offline.into(),
    )
}

fn start_join_with(
    invite: Invite,
    dir: &Path,
    bucket: &Bucket,
    name: &str,
    is_bot: bool,
    cancel: JoinCancel,
    options: WorkspaceOptions,
) -> JoinHandle<Result<Workspace>> {
    let setup = JoinSetup {
        connect: connector(bucket),
        options,
        cancel,
    };
    spawn_join(invite, dir, bucket, name, is_bot, setup)
}

/// How a test member joins: how it reaches the bucket once in, with what
/// options, and what can stop it while it waits.
struct JoinSetup {
    connect: Connector,
    options: WorkspaceOptions,
    cancel: JoinCancel,
}

/// Joins in the background, posting the request to `bucket`.
fn spawn_join(
    invite: Invite,
    dir: &Path,
    bucket: &Bucket,
    name: &str,
    is_bot: bool,
    setup: JoinSetup,
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
            setup.connect,
            setup.options,
            &setup.cancel,
        )
        .await
    })
}

/// Like `join`, the new member reaching the bucket through `connect`.
pub async fn join_connected(
    admin: &Workspace,
    dir: &Path,
    bucket: &Bucket,
    connect: Connector,
    name: &str,
) -> Workspace {
    let invite = admin.create_test_invite(name, Duration::from_secs(600)).unwrap();
    let token = invite.token.clone();
    let setup = JoinSetup {
        connect,
        options: Networking::Offline.into(),
        cancel: JoinCancel::new(),
    };
    let joining = spawn_join(invite, dir, bucket, name, false, setup);
    wait_for_request(admin, &token).await;
    admin.approve_invite(&token).await.unwrap();
    joining.await.unwrap().unwrap()
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
    join_as(admin, dir, bucket, name, is_bot, Networking::Offline.into()).await
}

/// Like `join`, for a person, opening the workspace with `options`.
pub async fn join_with(
    admin: &Workspace,
    dir: &Path,
    bucket: &Bucket,
    name: &str,
    options: WorkspaceOptions,
) -> Workspace {
    join_as(admin, dir, bucket, name, false, options).await
}

async fn join_as(
    admin: &Workspace,
    dir: &Path,
    bucket: &Bucket,
    name: &str,
    is_bot: bool,
    options: WorkspaceOptions,
) -> Workspace {
    let invite = admin.create_test_invite(name, Duration::from_secs(600)).unwrap();
    let token = invite.token.clone();
    let joining = start_join_with(invite, dir, bucket, name, is_bot, JoinCancel::new(), options);
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

/// What a dishonest admin's app could do, skipping its own checks: stage a
/// removal with any authenticated data, then sign any wrapper around it.
/// The admin merges the staged commit, as it would after saving it.
pub fn stage_removal_unchecked(
    admin: &Workspace,
    member: &iroh::PublicKey,
    binding: &[u8],
) -> (u64, Vec<u8>) {
    let mut state = admin.state();
    let from_epoch = state.mls.epoch().unwrap();
    let commit = state.mls.remove_members(&[*member], binding, None).unwrap();
    state.mls.merge_pending_commit().unwrap();
    state.record_epoch(&admin.inner.config).unwrap();
    (from_epoch, commit)
}

/// The authenticated data an honest admin's app gives a commit with these lists.
pub fn lists_binding(admin: &Workspace, from_epoch: u64, frozen: &[FrozenAuthor]) -> Vec<u8> {
    let key = admin.state().store.epoch_key(from_epoch).unwrap().unwrap();
    commit::lists_hash(&key, from_epoch, frozen, &[])
        .unwrap()
        .to_vec()
}

/// The admin's signature around any MLS commit and frozen list.
pub fn wrap_as_admin(
    admin: &Workspace,
    from_epoch: u64,
    commit: Vec<u8>,
    frozen: Vec<FrozenAuthor>,
) -> Vec<u8> {
    commit::sign(&admin.inner.identity, from_epoch, commit, frozen).unwrap()
}

pub fn frozen_at_last_seq(admin: &Workspace, member: &iroh::PublicKey) -> FrozenAuthor {
    FrozenAuthor {
        member: *member,
        last_seq: admin.state().store.last_seq(member).unwrap(),
    }
}

/// The device map key a dishonest admin commits with.
pub enum MapKeyChoice {
    /// The admin's own digest of the map.
    Admins,
    Other([u8; 32]),
    /// No pre-shared key at all.
    Missing,
}

/// A device a dishonest admin's app adds with any certificates and any
/// device-map key, skipping its own checks. The admin signs a v3 wrapper,
/// saves it and settles, as its app would. Returns the epoch the commit
/// leaves and the wrapper, for the caller to put in the bucket.
pub fn stage_device_add_unchecked(
    admin: &Workspace,
    device: &iroh::SecretKey,
    dir: &Path,
    certificates: &[DeviceCertificate],
    map_key: MapKeyChoice,
) -> (u64, Vec<u8>) {
    std::fs::create_dir_all(dir).unwrap();
    let device_mls = Mls::open(&dir.join("device-mls.db"), TEST_DB_KEY, device, None, None).unwrap();
    let binding = device.sign(&binding_message(device_mls.signer_public()));
    let key_package = device_mls.key_package().unwrap();
    let mut state = admin.state();
    let (_, key_package) = state
        .mls
        .verify_key_package(&key_package, &binding.to_bytes())
        .unwrap();
    let from_epoch = state.mls.epoch().unwrap();
    let from_epoch_key = state.mls.epoch_key().unwrap();
    let lists = commit::lists_hash(&from_epoch_key, from_epoch, &[], certificates).unwrap();
    let map_key = match map_key {
        MapKeyChoice::Admins => Some(
            state
                .map_key(&admin.inner.config.workspace_id, from_epoch, &[])
                .unwrap(),
        ),
        MapKeyChoice::Other(key) => Some(key),
        MapKeyChoice::Missing => None,
    };
    let (mls_commit, _) = state
        .mls
        .add_member(key_package, &lists, map_key.as_ref())
        .unwrap();
    let signed = commit::sign_v3(
        &admin.inner.identity,
        &from_epoch_key,
        from_epoch,
        mls_commit,
        Vec::new(),
        certificates,
    )
    .unwrap();
    state.save_commit(from_epoch, &signed).unwrap();
    state.mls.merge_pending_commit().unwrap();
    state.record_epoch(&admin.inner.config).unwrap();
    (from_epoch, signed)
}

/// `device` vouched for by `voucher` as one of `person`'s, at `epoch`.
pub fn device_certificate(
    voucher: &iroh::SecretKey,
    person: &iroh::PublicKey,
    device: &iroh::SecretKey,
    epoch: u64,
) -> DeviceCertificate {
    let acceptance = device::accept(device, TEST_WORKSPACE_ID, person).unwrap();
    DeviceCertificate::vouch(
        voucher,
        TEST_WORKSPACE_ID,
        *person,
        device.public(),
        epoch,
        acceptance,
    )
    .unwrap()
}

/// Links a new device in `dir` to `voucher`'s person: the device asks, the
/// voucher vouches, the admin adds it, and the device finishes joining.
pub async fn link_device(admin: &Workspace, voucher: &Workspace, dir: &Path, bucket: &Bucket) -> Workspace {
    link_device_with(admin, voucher, dir, bucket, Networking::Offline.into()).await
}

/// Like `link_device`, opening the device with `options`.
pub async fn link_device_with(
    admin: &Workspace,
    voucher: &Workspace,
    dir: &Path,
    bucket: &Bucket,
    options: WorkspaceOptions,
) -> Workspace {
    let (enrollment, request) = begin_link(voucher, dir).await;
    let certificate = vouch(voucher, &request);
    let welcome = admin
        .add_device(&request.key_package, &request.binding, &certificate)
        .await
        .unwrap();
    enrollment
        .finish_with(welcome, connector(bucket), options)
        .await
        .unwrap()
}

/// A new device in `dir` asking to be `voucher`'s person's.
pub async fn begin_link(voucher: &Workspace, dir: &Path) -> (DeviceEnrollment, DeviceRequest) {
    DeviceEnrollment::begin(data_dir(dir), device_link(voucher))
        .await
        .unwrap()
}

/// What `voucher` tells a new device of its person's about the workspace.
pub fn device_link(voucher: &Workspace) -> DeviceLink {
    let config = &voucher.inner.config;
    let admin_mls_key = voucher.state().mls.signature_key_of(&config.admin).unwrap();
    DeviceLink {
        workspace_id: config.workspace_id.clone(),
        workspace_name: config.name.clone(),
        admin: config.admin,
        admin_mls_key,
        person: voucher.my_person(),
    }
}

/// `voucher`'s certificate for the device behind `request`, at its epoch.
pub fn vouch(voucher: &Workspace, request: &DeviceRequest) -> DeviceCertificate {
    let epoch = voucher.state().mls.epoch().unwrap();
    DeviceCertificate::vouch(
        &voucher.inner.identity,
        &voucher.inner.config.workspace_id,
        voucher.my_person(),
        request.device,
        epoch,
        request.acceptance.clone(),
    )
    .unwrap()
}

/// A commit a dishonest admin's app could make, skipping its own checks:
/// adding each of `new_devices` and removing `removals` at once, carrying
/// `certificates`. The admin signs a v3 wrapper, saves it and settles.
/// Returns the epoch it leaves and the wrapper, for the bucket.
pub fn stage_devices_change_unchecked(
    admin: &Workspace,
    new_devices: &[(&iroh::SecretKey, &Path)],
    removals: &[iroh::PublicKey],
    certificates: &[DeviceCertificate],
) -> (u64, Vec<u8>) {
    let frozen: Vec<FrozenAuthor> = removals
        .iter()
        .map(|member| frozen_at_last_seq(admin, member))
        .collect();
    let key_packages: Vec<_> = new_devices
        .iter()
        .map(|(device, dir)| {
            std::fs::create_dir_all(dir).unwrap();
            let device_mls = Mls::open(&dir.join("device-mls.db"), TEST_DB_KEY, device, None, None).unwrap();
            let binding = device.sign(&binding_message(device_mls.signer_public()));
            (device_mls.key_package().unwrap(), binding.to_bytes())
        })
        .collect();
    let mut state = admin.state();
    let adds = key_packages
        .iter()
        .map(|(key_package, binding)| state.mls.verify_key_package(key_package, binding).unwrap().1)
        .collect();
    let from_epoch = state.mls.epoch().unwrap();
    let from_epoch_key = state.mls.epoch_key().unwrap();
    let lists = commit::lists_hash(&from_epoch_key, from_epoch, &frozen, certificates).unwrap();
    let map_key = state
        .map_key(&admin.inner.config.workspace_id, from_epoch, &frozen)
        .unwrap();
    let mls_commit = state
        .mls
        .commit_adds_and_removals(adds, removals, &lists, &map_key)
        .unwrap();
    let signed = commit::sign_v3(
        &admin.inner.identity,
        &from_epoch_key,
        from_epoch,
        mls_commit,
        frozen,
        certificates,
    )
    .unwrap();
    state.save_commit(from_epoch, &signed).unwrap();
    state.mls.merge_pending_commit().unwrap();
    state.record_epoch(&admin.inner.config).unwrap();
    (from_epoch, signed)
}

pub fn epoch_of(member: &Workspace) -> u64 {
    member.state().mls.epoch().unwrap()
}
