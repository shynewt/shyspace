//! Creating, joining, reopening and restoring a workspace.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use iroh::SecretKey;
use tokio::sync::{Notify, broadcast, mpsc, watch};

use super::{Inner, Networking, Profile, State, Workspace};
use crate::{
    config::{self, WorkspaceConfig},
    identity,
    keys::{DeviceKey, KeyStorage},
    membership::{
        invite::{Answer, Grant, Invite, JoinMailbox, JoinRequest, ensure_not_expired},
        mls::{Mls, binding_message},
    },
    ops::{Op, new_owned_id, new_random_id, now_ms},
    pages::Edit,
    storage::ApplyContext,
    storage::Store,
    sync::SyncStatus,
    transport::{
        bucket::{BucketConfig, Connector, s3_connector},
        gossip::Net,
        presigned::PresignedMailbox,
    },
    views::safety_code,
};

pub(crate) const STORE_FILE: &str = "shy.db";
pub(crate) const MLS_FILE: &str = "mls.db";
const SIGNER_META: &str = "mls_signer_public";
const LOCK_FILE: &str = "shyspace.lock";
const ANSWER_POLL: Duration = Duration::from_secs(2);

/// Stops a join that is still waiting for the admin's answer. Once the
/// answer has arrived the join finishes, so nothing is ever left half joined.
/// The request stays in the admin's list until they decline it or it expires.
#[derive(Clone)]
pub struct JoinCancel(Arc<watch::Sender<bool>>);

impl JoinCancel {
    pub fn new() -> Self {
        Self(Arc::new(watch::Sender::new(false)))
    }

    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
}

impl Default for JoinCancel {
    fn default() -> Self {
        Self::new()
    }
}

const GETTING_STARTED: &str = "# Welcome to shyspace\n\n\
Everything here is end-to-end encrypted. Your bucket only ever stores ciphertext.\n\n\
- **Chat** in channels, react, and reply.\n\
- **Pages** are collaborative markdown. Type together in real time.\n\
- **Share a page** in chat by pasting its link; the card stays live.\n\
- **Agents** join with an invite code like anyone else.\n\n\
Press `Ctrl K` to search everything.\n";

/// Where a member's data lives, and where the key that encrypts it is kept.
#[derive(Debug, Clone)]
pub struct DataDir {
    pub path: PathBuf,
    pub keys: KeyStorage,
}

impl DataDir {
    pub fn new(path: impl Into<PathBuf>, keys: KeyStorage) -> Self {
        Self {
            path: path.into(),
            keys,
        }
    }

    pub(crate) fn store_path(&self) -> PathBuf {
        self.path.join(STORE_FILE)
    }

    pub(crate) fn mls_path(&self) -> PathBuf {
        self.path.join(MLS_FILE)
    }

    /// This device's safety code, creating its identity if needed. A joiner
    /// shows it while waiting, so the admin can check who is asking.
    pub async fn safety_code(&self) -> Result<String> {
        std::fs::create_dir_all(&self.path).with_context(|| format!("creating {}", self.path.display()))?;
        Ok(safety_code(&self.open_local().await?.identity.public()))
    }

    async fn open_local(&self) -> Result<Local> {
        let lock = self.lock()?;
        let device_key = self.keys.load_or_create(&self.path).await?;
        let store = Store::open(&self.store_path(), &device_key.database_key(STORE_FILE))?;
        let identity = identity::load_or_create(&store)?;
        Ok(Local {
            lock,
            store,
            identity,
            device_key,
        })
    }

    /// Two processes on one folder would corrupt it (and fight over the same
    /// network identity), so the folder is locked while it's open.
    fn lock(&self) -> Result<std::fs::File> {
        let path = self.path.join(LOCK_FILE);
        let file = std::fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                bail!("this workspace is already open in another shyspace window or `shy` process")
            }
            Err(std::fs::TryLockError::Error(error)) => {
                Err(error).with_context(|| format!("locking {}", path.display()))
            }
        }
    }
}

/// The decrypted local pieces every workspace starts from.
struct Local {
    lock: std::fs::File,
    store: Store,
    identity: SecretKey,
    device_key: DeviceKey,
}

impl Local {
    fn open_mls(&self, data: &DataDir, group_id: Option<&[u8]>) -> Result<Mls> {
        let signer = self.store.meta(SIGNER_META)?;
        let mls = Mls::open(
            &data.mls_path(),
            &self.device_key.database_key(MLS_FILE),
            &self.identity,
            signer.as_deref(),
            group_id,
        )?;
        self.store.set_meta(SIGNER_META, mls.signer_public())?;
        Ok(mls)
    }
}

impl Workspace {
    pub fn exists(dir: &Path) -> bool {
        WorkspaceConfig::exists(dir)
    }

    pub async fn open(data: &DataDir, networking: Networking) -> Result<Self> {
        Self::open_with(data, s3_connector(), networking).await
    }

    /// Creates a workspace, after checking the bucket works with `bucket`.
    pub async fn create(
        data: &DataDir,
        workspace_name: &str,
        profile: Profile<'_>,
        bucket: BucketConfig,
        networking: Networking,
    ) -> Result<Self> {
        Self::create_with(
            data,
            new_random_id(),
            workspace_name,
            profile,
            bucket,
            s3_connector(),
            networking,
        )
        .await
    }

    /// Asks the admin to let us in and waits until they decide or the invite
    /// expires. The admin approves from their app.
    pub async fn join(
        data: &DataDir,
        invite_code: &str,
        profile: Profile<'_>,
        networking: Networking,
    ) -> Result<Self> {
        Self::join_cancellable(data, invite_code, profile, networking, &JoinCancel::new()).await
    }

    /// Like [`Workspace::join`], but `cancel` stops it while it waits.
    pub async fn join_cancellable(
        data: &DataDir,
        invite_code: &str,
        profile: Profile<'_>,
        networking: Networking,
        cancel: &JoinCancel,
    ) -> Result<Self> {
        let invite = Invite::decode(invite_code)?;
        let mailbox = PresignedMailbox::for_invite(&invite);
        Self::join_with(
            data,
            invite,
            profile,
            &mailbox,
            s3_connector(),
            networking,
            cancel,
        )
        .await
    }

    pub(crate) async fn create_with(
        data: &DataDir,
        workspace_id: String,
        workspace_name: &str,
        profile: Profile<'_>,
        bucket_config: BucketConfig,
        connect: Connector,
        networking: Networking,
    ) -> Result<Self> {
        let name = super::actions::checked_profile_name(profile.name)?;
        let bucket = connect(&workspace_id, &bucket_config)?;
        bucket
            .check_access()
            .await
            .context("checking the bucket settings")?;
        prepare_empty_dir(&data.path)?;
        let local = data.open_local().await?;
        let mut mls = local.open_mls(data, None)?;
        mls.create_group(workspace_id.as_bytes())?;
        let config = WorkspaceConfig {
            workspace_id,
            name: workspace_name.to_owned(),
            admin: local.identity.public(),
            bucket: bucket_config,
        };
        config.store_bucket(&local.store)?;
        let workspace = Self::start(data, local, mls, config, connect, networking).await?;
        workspace.write(Op::Profile {
            name,
            is_bot: profile.is_bot,
        })?;
        workspace.write(Op::ChannelCreate {
            id: new_owned_id(&workspace.me()),
            name: "general".into(),
        })?;
        let page = workspace.create_page("Getting started", None)?;
        workspace.edit_page(&page, Edit::Append(GETTING_STARTED))?;
        workspace.inner.config.mark_complete(&data.path)?;
        Ok(workspace)
    }

    pub(crate) async fn join_with(
        data: &DataDir,
        invite: Invite,
        profile: Profile<'_>,
        mailbox: &impl JoinMailbox,
        connect: Connector,
        networking: Networking,
        cancel: &JoinCancel,
    ) -> Result<Self> {
        ensure_not_expired(&invite, now_ms()?)?;
        // Checked before the admin is asked: refused later, it would leave a
        // spent invite and a half-made workspace.
        let name = super::actions::checked_profile_name(profile.name)?;
        prepare_empty_dir(&data.path)?;
        let local = data.open_local().await?;
        let mut mls = local.open_mls(data, None)?;
        let request = JoinRequest {
            key_package: mls.key_package()?,
            binding: local
                .identity
                .sign(&binding_message(mls.signer_public()))
                .to_bytes()
                .to_vec(),
            name: name.clone(),
            is_bot: profile.is_bot,
        };
        mailbox.post_request(postcard::to_stdvec(&request)?).await?;
        let (welcome, sealed_grant) = wait_for_answer(&invite, mailbox, cancel).await?;
        mls.join(
            &welcome,
            invite.workspace_id.as_bytes(),
            &invite.admin,
            &invite.admin_mls_key,
        )?;
        let grant: Grant = postcard::from_bytes(&mls.open_from_admin(&sealed_grant, &invite.admin)?)?;
        grant.history.store_into(
            &local.store,
            ApplyContext {
                workspace_id: &invite.workspace_id,
                admin: invite.admin,
                me: local.identity.public(),
            },
        )?;
        let bucket_lamport = grant.bucket_lamport;

        let config = WorkspaceConfig {
            workspace_id: invite.workspace_id,
            name: invite.workspace_name,
            admin: invite.admin,
            bucket: grant.bucket,
        };
        config.store_bucket(&local.store)?;
        config::update_bucket(&local.store, &config.bucket, bucket_lamport)?;
        let workspace = Self::start(data, local, mls, config, connect, networking).await?;
        workspace.write(Op::Profile {
            name,
            is_bot: profile.is_bot,
        })?;
        workspace.inner.config.mark_complete(&data.path)?;
        Ok(workspace)
    }

    pub(crate) async fn open_with(
        data: &DataDir,
        connect: Connector,
        networking: Networking,
    ) -> Result<Self> {
        ensure!(
            Self::exists(&data.path),
            "no workspace in {}",
            data.path.display()
        );
        let local = data.open_local().await?;
        let config = WorkspaceConfig::load(&data.path, &local.store)?;
        ensure!(
            local.store.meta(SIGNER_META)?.is_some(),
            "the MLS signing key reference is missing"
        );
        let mls = local.open_mls(data, Some(config.workspace_id.as_bytes()))?;
        Self::start(data, local, mls, config, connect, networking).await
    }

    async fn start(
        data: &DataDir,
        local: Local,
        mls: Mls,
        config: WorkspaceConfig,
        connect: Connector,
        networking: Networking,
    ) -> Result<Self> {
        let bucket = connect(&config.workspace_id, &config.bucket)?;
        let Local {
            lock,
            store,
            identity,
            device_key,
        } = local;
        let mut state = State { store, mls };
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
        let appear_offline = super::presence::load_appear_offline(&state.store)?;
        let workspace = Self {
            inner: Arc::new(Inner {
                data: data.clone(),
                _data_lock: lock,
                device_key,
                identity,
                config,
                state: Mutex::new(state),
                bucket: RwLock::new(bucket),
                connect,
                net,
                events: broadcast::channel(1024).0,
                wake: Notify::new(),
                sync_lock: tokio::sync::Mutex::new(()),
                recovery_lock: tokio::sync::Mutex::new(()),
                status: Mutex::new(SyncStatus::default()),
                refetched_gaps: Mutex::new(std::collections::HashSet::new()),
                seen_fork_proofs: Mutex::new(std::collections::HashMap::new()),
                last_dialed_ms: Mutex::new(std::collections::HashMap::new()),
                presence: Mutex::new(Default::default()),
                own_presence: Mutex::new(super::presence::OwnPresence {
                    is_away: false,
                    appear_offline,
                    last_sent: None,
                }),
                heartbeat_now: Notify::new(),
                stopped: tokio::sync::watch::Sender::new(false),
                #[cfg(test)]
                fail_next_apply: Default::default(),
                #[cfg(test)]
                fail_next_park: Default::default(),
            }),
        };
        workspace
            .state()
            .store
            .rebuild_if_rules_changed(workspace.apply_context())?;
        workspace.announce_sealing_key()?;
        if let Some(net) = &workspace.inner.net {
            net.set_members(&workspace.current_members()?);
            workspace.spawn_background(incoming);
        }
        Ok(workspace)
    }
}

/// Polls for the admin's answer until the invite expires.
async fn wait_for_answer(
    invite: &Invite,
    mailbox: &impl JoinMailbox,
    cancel: &JoinCancel,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut cancelled = cancel.0.subscribe();
    loop {
        // Fetching changes nothing on disk, so a cancel needn't wait for a slow one.
        let fetched = tokio::select! {
            fetched = mailbox.fetch_answer() => fetched?,
            _ = cancelled.wait_for(|is_cancelled| *is_cancelled) => bail!("you cancelled the join request"),
        };
        // Undecodable bytes are junk from someone with bucket access; the
        // admin re-sends the real answer, so keep polling.
        if let Some(bytes) = fetched
            && let Ok(answer) = postcard::from_bytes::<Answer>(&bytes)
        {
            match answer {
                Answer::Accepted {
                    welcome,
                    sealed_grant,
                } => return Ok((welcome, sealed_grant)),
                Answer::Rejected => bail!("the admin declined this request"),
            }
        }
        ensure!(
            now_ms()? < invite.expires_ms,
            "the invite expired before the admin answered"
        );
        tokio::select! {
            () = tokio::time::sleep(ANSWER_POLL) => {}
            _ = cancelled.wait_for(|is_cancelled| *is_cancelled) => bail!("you cancelled the join request"),
        }
    }
}

fn prepare_empty_dir(dir: &Path) -> Result<()> {
    if WorkspaceConfig::exists(dir) {
        bail!("a workspace already exists in {}", dir.display());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))
}

/// The gossip topic is derived from the workspace id, so only people who know
/// the id (from an invite) find the swarm. Content is encrypted regardless.
fn gossip_topic(workspace_id: &str) -> [u8; 32] {
    blake3::derive_key("shyspace gossip topic v1", workspace_id.as_bytes())
}
