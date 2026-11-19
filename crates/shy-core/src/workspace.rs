//! The public entry point: one workspace, stored in one data directory.

use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use iroh::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, mpsc};
use tracing::warn;

use crate::{
    bucket::{Bucket, BucketConfig, JoinRequest},
    crypto,
    docs::{self, Edit},
    invite::Invite,
    mls::{Mls, binding_message},
    net::Net,
    ops::{EntityId, Envelope, Op, OpBody, VerifiedOp, new_owned_id, new_random_id},
    store::Store,
    sync::{KeyringEntry, SyncStatus},
    views::{Change, Channel, Member, Message, Page, SearchHit, member_id},
};

const CONFIG_FILE: &str = "workspace.json";
const IDENTITY_FILE: &str = "identity.key";
const SIGNER_META: &str = "mls_signer_public";
const WELCOME_POLL: Duration = Duration::from_secs(2);
const KEYRING_TIMEOUT: Duration = Duration::from_secs(120);

const GETTING_STARTED: &str = "# Welcome to shyspace\n\n\
Everything here is end-to-end encrypted. Your bucket only ever stores ciphertext.\n\n\
- **Chat** in channels, react, and reply.\n\
- **Pages** are collaborative markdown. Type together in real time.\n\
- **Share a page** in chat by pasting its link; the card stays live.\n\
- **Agents** join with an invite code like anyone else.\n\n\
Press `Ctrl K` to search everything.\n";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub workspace_id: String,
    pub name: String,
    pub admin: PublicKey,
    pub bucket: BucketConfig,
    /// Set only on the admin's device: the token join requests must carry.
    pub invite_token: Option<String>,
}

pub struct Profile<'a> {
    pub name: &'a str,
    pub is_bot: bool,
}

pub(crate) struct State {
    pub store: Store,
    pub mls: Mls,
}

impl State {
    /// After a crash, finishes a staged commit only if it was signed and
    /// saved (so it can reach the bucket); otherwise forgets it.
    fn recover_staged_commit(&mut self) -> Result<()> {
        if !self.mls.has_pending_commit() {
            return Ok(());
        }
        if self.store.has_commit_record(self.mls.epoch()?)? {
            self.mls.merge_pending_commit()?;
        } else {
            self.mls.discard_pending_commit()?;
            crate::sync::drop_unsealed_deliveries(&self.store)?;
        }
        Ok(())
    }

    /// Stores the current epoch's key and member list after any epoch change.
    pub fn record_epoch(&self) -> Result<()> {
        if self.mls.is_active() {
            let members = self.mls.members()?;
            self.store
                .put_epoch(self.mls.epoch()?, &self.mls.epoch_key()?, &members)?;
            self.store.set_members(&members)?;
        }
        Ok(())
    }
}

pub(crate) struct Inner {
    pub identity: SecretKey,
    pub config: WorkspaceConfig,
    pub state: Mutex<State>,
    pub bucket: Bucket,
    pub net: Option<Net>,
    pub events: broadcast::Sender<Change>,
    pub wake: Notify,
    pub sync_lock: tokio::sync::Mutex<()>,
    pub status: Mutex<SyncStatus>,
}

#[derive(Clone)]
pub struct Workspace {
    pub(crate) inner: Arc<Inner>,
}

/// Whether to go online. Tests run several members against one in-memory
/// bucket without networking.
#[derive(Clone, Copy, PartialEq)]
pub enum Networking {
    Online,
    Offline,
}

fn load_or_create_identity(dir: &Path) -> Result<SecretKey> {
    let path = dir.join(IDENTITY_FILE);
    if path.exists() {
        let bytes: [u8; 32] = std::fs::read(&path)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("corrupt identity key"))?;
        return Ok(SecretKey::from_bytes(&bytes));
    }
    let identity = SecretKey::generate();
    write_private_file(&path, &identity.to_bytes())?;
    Ok(identity)
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)?.write_all(contents)?;
    Ok(())
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

pub fn parse_member_id(id: &str) -> Result<PublicKey> {
    let bytes: [u8; 32] = hex::decode(id)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("member id must be 32 bytes"))?;
    Ok(PublicKey::from_bytes(&bytes)?)
}

impl Workspace {
    pub fn exists(dir: &Path) -> bool {
        dir.join(CONFIG_FILE).exists()
    }

    pub async fn open(dir: &Path, networking: Networking) -> Result<Self> {
        let config: WorkspaceConfig = serde_json::from_slice(&std::fs::read(dir.join(CONFIG_FILE))?)?;
        let bucket = config.bucket.connect(&config.workspace_id)?;
        Self::open_with(dir, bucket, networking).await
    }

    pub async fn create(
        dir: &Path,
        workspace_name: &str,
        profile: Profile<'_>,
        bucket: BucketConfig,
        networking: Networking,
    ) -> Result<Self> {
        let workspace_id = new_random_id();
        let connected = bucket.connect(&workspace_id)?;
        Self::create_with(
            dir,
            workspace_id,
            workspace_name,
            profile,
            bucket,
            connected,
            networking,
        )
        .await
    }

    pub(crate) async fn create_with(
        dir: &Path,
        workspace_id: String,
        workspace_name: &str,
        profile: Profile<'_>,
        bucket_config: BucketConfig,
        bucket: Bucket,
        networking: Networking,
    ) -> Result<Self> {
        if Self::exists(dir) {
            bail!("a workspace already exists in {}", dir.display());
        }
        std::fs::create_dir_all(dir)?;
        let identity = load_or_create_identity(dir)?;
        let store = Store::open(&dir.join("shy.db"))?;
        let mut mls = Mls::open(&dir.join("mls.db"), &identity, None, None)?;
        mls.create_group(workspace_id.as_bytes())?;
        store.set_meta(SIGNER_META, mls.signer_public())?;
        let config = WorkspaceConfig {
            workspace_id,
            name: workspace_name.to_owned(),
            admin: identity.public(),
            bucket: bucket_config,
            invite_token: Some(new_random_id()),
        };
        write_private_file(&dir.join(CONFIG_FILE), &serde_json::to_vec_pretty(&config)?)?;
        let workspace = Self::start(identity, config, State { store, mls }, bucket, networking).await?;
        workspace.write(Op::Profile {
            name: profile.name.to_owned(),
            is_bot: profile.is_bot,
        })?;
        workspace.write(Op::ChannelCreate {
            id: new_owned_id(&workspace.me()),
            name: "general".into(),
        })?;
        let page = workspace.create_page("Getting started", None)?;
        workspace.edit_page(&page, Edit::Append(GETTING_STARTED))?;
        Ok(workspace)
    }

    /// Asks the admin to let us in and waits until they do. The admin's app
    /// accepts automatically the next time it is online.
    pub async fn join(
        dir: &Path,
        invite_code: &str,
        profile: Profile<'_>,
        networking: Networking,
    ) -> Result<Self> {
        let invite = Invite::decode(invite_code)?;
        let bucket = invite.bucket.connect(&invite.workspace_id)?;
        Self::join_with(dir, invite, profile, bucket, networking).await
    }

    pub(crate) async fn join_with(
        dir: &Path,
        invite: Invite,
        profile: Profile<'_>,
        bucket: Bucket,
        networking: Networking,
    ) -> Result<Self> {
        if Self::exists(dir) {
            bail!("a workspace already exists in {}", dir.display());
        }
        std::fs::create_dir_all(dir)?;
        let identity = load_or_create_identity(dir)?;
        let store = Store::open(&dir.join("shy.db"))?;
        let signer = store.meta(SIGNER_META)?;
        let mut mls = Mls::open(&dir.join("mls.db"), &identity, signer.as_deref(), None)?;
        store.set_meta(SIGNER_META, mls.signer_public())?;

        let request = JoinRequest {
            key_package: mls.key_package()?,
            binding: identity
                .sign(&binding_message(mls.signer_public()))
                .to_bytes()
                .to_vec(),
        };
        bucket
            .request_join(&invite.token, &identity.public(), &request)
            .await?;
        let welcome = loop {
            if let Some(welcome) = bucket.welcome(&identity.public()).await? {
                break welcome;
            }
            tokio::time::sleep(WELCOME_POLL).await;
        };
        mls.join(
            &welcome,
            invite.workspace_id.as_bytes(),
            &invite.admin,
            &invite.admin_mls_key,
        )?;
        let epoch = mls.epoch()?;
        // The admin re-uploads the key history until we post our first op,
        // so an overwritten copy gets repaired; give up if it never does.
        let deadline = tokio::time::Instant::now() + KEYRING_TIMEOUT;
        let keyring: Vec<KeyringEntry> = loop {
            if let Some(sealed) = bucket.keyring(epoch).await?
                && let Ok(payload) = mls.open_from_admin(&sealed, &invite.admin)
            {
                break postcard::from_bytes(&payload)?;
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "the admin's key history never arrived; ask for a new invite"
            );
            tokio::time::sleep(WELCOME_POLL).await;
        };
        for entry in keyring {
            store.put_epoch(entry.epoch, &entry.key, &entry.members)?;
        }

        let config = WorkspaceConfig {
            workspace_id: invite.workspace_id,
            name: invite.workspace_name,
            admin: invite.admin,
            bucket: invite.bucket,
            invite_token: None,
        };
        write_private_file(&dir.join(CONFIG_FILE), &serde_json::to_vec_pretty(&config)?)?;
        let workspace = Self::start(identity, config, State { store, mls }, bucket, networking).await?;
        workspace.write(Op::Profile {
            name: profile.name.to_owned(),
            is_bot: profile.is_bot,
        })?;
        Ok(workspace)
    }

    pub(crate) async fn open_with(dir: &Path, bucket: Bucket, networking: Networking) -> Result<Self> {
        let config: WorkspaceConfig = serde_json::from_slice(&std::fs::read(dir.join(CONFIG_FILE))?)?;
        let identity = load_or_create_identity(dir)?;
        let store = Store::open(&dir.join("shy.db"))?;
        let signer = store
            .meta(SIGNER_META)?
            .context("MLS signing key reference missing")?;
        let mls = Mls::open(
            &dir.join("mls.db"),
            &identity,
            Some(&signer),
            Some(config.workspace_id.as_bytes()),
        )?;
        Self::start(identity, config, State { store, mls }, bucket, networking).await
    }

    async fn start(
        identity: SecretKey,
        config: WorkspaceConfig,
        mut state: State,
        bucket: Bucket,
        networking: Networking,
    ) -> Result<Self> {
        state.recover_staged_commit()?;
        state.record_epoch()?;
        let (incoming_sender, incoming) = mpsc::channel(1024);
        let net = match networking {
            Networking::Online => {
                let topic = gossip_topic(&config.workspace_id);
                Some(Net::start(identity.clone(), topic, incoming_sender).await?)
            }
            Networking::Offline => None,
        };
        let workspace = Self {
            inner: Arc::new(Inner {
                identity,
                config,
                state: Mutex::new(state),
                bucket,
                net,
                events: broadcast::channel(1024).0,
                wake: Notify::new(),
                sync_lock: tokio::sync::Mutex::new(()),
                status: Mutex::new(SyncStatus::default()),
            }),
        };
        if networking == Networking::Online {
            workspace.spawn_background(incoming);
        }
        Ok(workspace)
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().expect("workspace state lock poisoned")
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.inner.events.subscribe()
    }

    pub(crate) fn emit(&self, change: Change) {
        let _ = self.inner.events.send(change);
    }

    pub fn me(&self) -> PublicKey {
        self.inner.identity.public()
    }

    pub fn name(&self) -> &str {
        &self.inner.config.name
    }

    pub fn is_admin(&self) -> bool {
        self.inner.config.admin == self.me()
    }

    pub fn invite_code(&self) -> Result<String> {
        let config = &self.inner.config;
        let token = config
            .invite_token
            .clone()
            .context("only the workspace admin can invite")?;
        Invite {
            workspace_id: config.workspace_id.clone(),
            workspace_name: config.name.clone(),
            admin: config.admin,
            admin_mls_key: self.state().mls.signer_public().to_vec(),
            token,
            bucket: config.bucket.clone(),
        }
        .encode()
    }

    /// Seals, stores and applies a local change, then sends it live. The
    /// bucket upload happens on the next sync pass.
    pub fn write(&self, op: Op) -> Result<()> {
        let envelope = {
            let state = self.state();
            if !state.mls.is_active() {
                bail!("you are no longer a member of this workspace");
            }
            let epoch = state.mls.epoch()?;
            let key = state
                .store
                .epoch_key(epoch)?
                .context("missing key for the current epoch")?;
            let author = self.me();
            let body = OpBody {
                lamport: state.store.next_lamport()?,
                timestamp_ms: now_ms(),
                op,
            };
            let seq = state.store.next_seq(&author)?;
            let envelope = crypto::seal(&self.inner.identity, &key, epoch, seq, &body)?;
            state.store.insert_applied(&envelope, false)?;
            let change = state.store.apply(&VerifiedOp { author, seq, body })?;
            drop(state);
            self.emit(change);
            envelope
        };
        self.send_live(envelope);
        self.inner.wake.notify_one();
        Ok(())
    }

    fn send_live(&self, envelope: Envelope) {
        let workspace = self.clone();
        if self.inner.net.is_some() {
            tokio::spawn(async move {
                if let Some(net) = &workspace.inner.net
                    && let Err(error) = net.broadcast(envelope).await
                {
                    warn!("live send failed, the bucket will deliver it: {error}");
                }
            });
        }
    }

    pub fn create_channel(&self, name: &str) -> Result<EntityId> {
        let id = new_owned_id(&self.me());
        let name = name
            .trim()
            .trim_start_matches('#')
            .to_lowercase()
            .replace(' ', "-");
        if name.is_empty() {
            bail!("channel name is empty");
        }
        self.write(Op::ChannelCreate { id: id.clone(), name })?;
        Ok(id)
    }

    pub fn send_message(&self, channel: &str, text: &str, reply_to: Option<EntityId>) -> Result<EntityId> {
        let text = text.trim();
        if text.is_empty() {
            bail!("message is empty");
        }
        let id = new_owned_id(&self.me());
        self.write(Op::Message {
            id: id.clone(),
            channel: channel.to_owned(),
            text: text.to_owned(),
            reply_to,
        })?;
        Ok(id)
    }

    pub fn react(&self, message: &str, emoji: &str, active: bool) -> Result<()> {
        self.write(Op::Reaction {
            message: message.to_owned(),
            emoji: emoji.to_owned(),
            active,
        })
    }

    pub fn create_page(&self, title: &str, parent: Option<EntityId>) -> Result<EntityId> {
        let id = new_owned_id(&self.me());
        self.write(Op::PageCreate {
            id: id.clone(),
            parent,
            title: title.trim().to_owned(),
        })?;
        Ok(id)
    }

    pub fn rename_page(&self, page: &str, title: &str) -> Result<()> {
        self.write(Op::PageRename {
            id: page.to_owned(),
            title: title.trim().to_owned(),
        })
    }

    /// A Yjs update produced by an editor.
    pub fn apply_doc_update(&self, page: &str, update: Vec<u8>) -> Result<()> {
        self.write(Op::DocUpdate {
            page: page.to_owned(),
            update,
        })
    }

    pub fn edit_page(&self, page: &str, edit: Edit) -> Result<()> {
        let state = self.state().store.page_state(page)?;
        let update = docs::edit(state.as_deref(), edit)?;
        self.apply_doc_update(page, update)
    }

    pub fn set_profile(&self, profile: Profile<'_>) -> Result<()> {
        self.write(Op::Profile {
            name: profile.name.trim().to_owned(),
            is_bot: profile.is_bot,
        })
    }

    pub fn channels(&self) -> Result<Vec<Channel>> {
        self.state().store.channels()
    }

    pub fn messages(&self, channel: &str, limit: u32) -> Result<Vec<Message>> {
        self.state().store.messages(channel, &self.me(), limit)
    }

    pub fn message_channel(&self, message: &str) -> Result<Option<EntityId>> {
        self.state().store.message_channel(message)
    }

    pub fn pages(&self) -> Result<Vec<Page>> {
        self.state().store.pages()
    }

    pub fn page_state(&self, page: &str) -> Result<Option<Vec<u8>>> {
        self.state().store.page_state(page)
    }

    pub fn page_text(&self, page: &str) -> Result<String> {
        docs::read(self.page_state(page)?.as_deref())
    }

    pub fn members(&self) -> Result<Vec<Member>> {
        self.state().store.members(&self.inner.config.admin)
    }

    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        self.state().store.search(query, limit)
    }

    pub fn my_id(&self) -> String {
        member_id(&self.me())
    }

    pub async fn shutdown(&self) {
        if let Some(net) = &self.inner.net {
            net.shutdown().await;
        }
    }
}

/// The gossip topic is derived from the workspace id, so only people who know
/// the id (from an invite) find the swarm. Content is encrypted regardless.
fn gossip_topic(workspace_id: &str) -> [u8; 32] {
    blake3::derive_key("shyspace gossip topic v1", workspace_id.as_bytes())
}
