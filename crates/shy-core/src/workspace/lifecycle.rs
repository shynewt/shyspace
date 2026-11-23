//! Creating, joining, reopening and restoring a workspace.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use iroh::SecretKey;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, mpsc, watch};

use super::{Inner, Networking, Profile, State, Workspace, WorkspaceOptions};
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
    storage::{Store, VIEW_RULES, VIEW_RULES_META},
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
/// The name chosen when joining or creating, until its Profile op is
/// written: a join the admin answered, or a create cut short, finishes on
/// the next start rather than being undone.
const PENDING_PROFILE_META: &str = "pending_profile";
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
    pub(crate) fn lock(&self) -> Result<std::fs::File> {
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

    pub async fn open(data: &DataDir, options: impl Into<WorkspaceOptions>) -> Result<Self> {
        Self::open_with(data, s3_connector(), options).await
    }

    /// Creates a workspace, after checking the bucket works with `bucket`.
    pub async fn create(
        data: &DataDir,
        workspace_name: &str,
        profile: Profile<'_>,
        bucket: BucketConfig,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Self> {
        Self::create_with(
            data,
            new_random_id(),
            workspace_name,
            profile,
            bucket,
            s3_connector(),
            options,
        )
        .await
    }

    /// Asks the admin to let us in and waits until they decide or the invite
    /// expires. The admin approves from their app.
    pub async fn join(
        data: &DataDir,
        invite_code: &str,
        profile: Profile<'_>,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Self> {
        Self::join_cancellable(data, invite_code, profile, options, &JoinCancel::new()).await
    }

    /// Like [`Workspace::join`], but `cancel` stops it while it waits.
    pub async fn join_cancellable(
        data: &DataDir,
        invite_code: &str,
        profile: Profile<'_>,
        options: impl Into<WorkspaceOptions>,
        cancel: &JoinCancel,
    ) -> Result<Self> {
        let invite = Invite::decode(invite_code)?;
        let mailbox = PresignedMailbox::for_invite(&invite);
        Self::join_with(data, invite, profile, &mailbox, s3_connector(), options, cancel).await
    }

    pub(crate) async fn create_with(
        data: &DataDir,
        workspace_id: String,
        workspace_name: &str,
        profile: Profile<'_>,
        bucket_config: BucketConfig,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
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
        config.save_in_store(&local.store)?;
        let pending = PendingProfile {
            name,
            is_bot: profile.is_bot,
        };
        local
            .store
            .set_meta(PENDING_PROFILE_META, &serde_json::to_vec(&pending)?)?;
        config.mark_complete(&data.path)?;
        let workspace = Self::start(data, local, mls, config, connect, options).await?;
        #[cfg(test)]
        if CUT_SHORT_CREATES
            .lock()
            .expect("cut-short creates lock poisoned")
            .contains(&data.path)
        {
            bail!("a simulated crash while creating");
        }
        workspace.finish_pending_profile()?;
        workspace.write(Op::ChannelCreate {
            id: new_owned_id(&workspace.me()),
            name: "general".into(),
        })?;
        let page = workspace.create_page("Getting started", None)?;
        workspace.edit_page(&page, Edit::Append(GETTING_STARTED))?;
        Ok(workspace)
    }

    pub(crate) async fn join_with(
        data: &DataDir,
        invite: Invite,
        profile: Profile<'_>,
        mailbox: &impl JoinMailbox,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
        cancel: &JoinCancel,
    ) -> Result<Self> {
        ensure_not_expired(&invite, now_ms()?)?;
        // Checked before the admin is asked: refused later, it would leave a
        // spent invite and a half-made workspace.
        let name = super::actions::checked_profile_name(profile.name)?;
        ensure!(!name.is_empty(), "a name is needed to join");
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
        config.save_in_store(&local.store)?;
        config::update_bucket(&local.store, &config.bucket, bucket_lamport)?;
        let pending = PendingProfile {
            name,
            is_bot: profile.is_bot,
        };
        local
            .store
            .set_meta(PENDING_PROFILE_META, &serde_json::to_vec(&pending)?)?;
        config.mark_complete(&data.path)?;
        let workspace = Self::start(data, local, mls, config, connect, options).await?;
        workspace.finish_pending_profile()?;
        Ok(workspace)
    }

    pub(crate) async fn open_with(
        data: &DataDir,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
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
        connect(&config.workspace_id, &config.bucket).context(
            "the bucket settings this workspace has (from its admin) can't be used; ask the workspace admin to check them in Settings › Storage",
        )?;
        let mls = local.open_mls(data, Some(config.workspace_id.as_bytes()))?;
        let workspace = Self::start(data, local, mls, config, connect, options).await?;
        workspace.finish_pending_profile()?;
        Ok(workspace)
    }

    /// Writes the Profile op a join or create chose, if it didn't get
    /// written then.
    fn finish_pending_profile(&self) -> Result<()> {
        let Some(bytes) = self.state().store.meta(PENDING_PROFILE_META)? else {
            return Ok(());
        };
        let pending: PendingProfile =
            serde_json::from_slice(&bytes).context("reading the name chosen when joining or creating")?;
        if self.state().mls.is_active() {
            let written = self.write(Op::Profile {
                name: pending.name,
                is_bot: pending.is_bot,
            });
            // Removed in the meantime (sync runs alongside): nothing to write.
            if let Err(error) = written
                && self.state().mls.is_active()
            {
                return Err(error);
            }
        }
        self.state().store.delete_meta(PENDING_PROFILE_META)
    }

    async fn start(
        data: &DataDir,
        local: Local,
        mls: Mls,
        config: WorkspaceConfig,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
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
        let options: WorkspaceOptions = options.into();
        let net = match options.networking {
            Networking::Online => {
                let topic = gossip_topic(&config.workspace_id);
                Some(
                    Net::start(
                        identity.clone(),
                        topic,
                        incoming_sender,
                        options.protocols.as_ref(),
                    )
                    .await?,
                )
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
                huddle_media: options.media.clone(),
                config,
                state: Mutex::new(state),
                bucket: RwLock::new(bucket),
                connect,
                net,
                events: broadcast::channel(1024).0,
                wake: Notify::new(),
                sync_lock: tokio::sync::Mutex::new(()),
                push_now: Notify::new(),
                push_lock: tokio::sync::Mutex::new(()),
                answers_sent: Default::default(),
                recovery_lock: tokio::sync::Mutex::new(()),
                status: Mutex::new(SyncStatus::default()),
                refetched_gaps: Mutex::new(std::collections::HashSet::new()),
                skipped_segments: Mutex::new(std::collections::HashSet::new()),
                fetch_failures: Mutex::new(std::collections::HashMap::new()),
                last_slow_duties: Mutex::new(None),
                verified_commits: Mutex::new(std::collections::HashMap::new()),
                removal_reads: Mutex::new(std::collections::HashMap::new()),
                seen_heads: Mutex::new(std::collections::HashMap::new()),
                confirmed_head_seq: Default::default(),
                heard_hints: Mutex::new(std::collections::HashMap::new()),
                relays: Mutex::new(Default::default()),
                cadence: Mutex::new(Default::default()),
                seen_fork_proofs: Mutex::new(std::collections::HashMap::new()),
                last_dialed_ms: Mutex::new(std::collections::HashMap::new()),
                presence: Mutex::new(Default::default()),
                huddles: Mutex::new(Default::default()),
                huddle_transition: Mutex::new(()),
                runtime: tokio::runtime::Handle::current(),
                own_presence: Mutex::new(super::presence::OwnPresence {
                    is_away: false,
                    appear_offline,
                    last_sent: None,
                }),
                heartbeat_now: Notify::new(),
                file_cache_size: Default::default(),
                stopped: tokio::sync::watch::Sender::new(false),
                #[cfg(test)]
                fail_next_apply: Default::default(),
                #[cfg(test)]
                fail_next_park: Default::default(),
                #[cfg(test)]
                fail_segment_save: Default::default(),
                #[cfg(test)]
                drop_segment_transaction: Default::default(),
                #[cfg(test)]
                fail_next_answer_upload: Default::default(),
            }),
        };
        workspace.rebuild_views_if_rules_changed()?;
        workspace.announce_sealing_key()?;
        if let Some(net) = &workspace.inner.net {
            net.set_members(&workspace.current_members()?);
            workspace.spawn_background(incoming);
        }
        Ok(workspace)
    }
}

impl Workspace {
    /// Rebuilds the views from the log when they were built under other
    /// rules for showing ops (by another version of the app).
    fn rebuild_views_if_rules_changed(&self) -> Result<()> {
        let state = self.state();
        if state.store.meta(VIEW_RULES_META)?.as_deref() == Some([VIEW_RULES].as_slice()) {
            return Ok(());
        }
        let context = self.apply_context();
        state.store.atomically(|store| {
            store.rebuild_views(context)?;
            store.set_meta(VIEW_RULES_META, &[VIEW_RULES])
        })
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

/// Folders whose create stops just after it is complete, as if the app
/// died then; keyed by folder so tests running alongside aren't touched.
#[cfg(test)]
pub(crate) static CUT_SHORT_CREATES: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

#[derive(Serialize, Deserialize)]
struct PendingProfile {
    name: String,
    is_bot: bool,
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
