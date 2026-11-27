//! Creating, joining, reopening and restoring a workspace.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use iroh::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, mpsc, watch};
use tracing::warn;

use super::{Inner, Networking, Profile, State, Workspace, WorkspaceOptions};
use crate::{
    config::{self, WorkspaceConfig},
    identity,
    keys::{DeviceKey, KeyStorage},
    membership::{
        device::map_digest,
        invite::{
            Answer, Grant, GrantWithDevices, Invite, JoinMailbox, JoinRequest, accepted_bytes,
            declined_bytes, ensure_not_expired,
        },
        mls::{Mls, binding_message},
        request_seal::seal_request,
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

    /// Gives this device `identity` before it joins, for tests that need a chosen key.
    #[cfg(test)]
    pub(crate) async fn use_identity_for_test(&self, identity: &iroh::SecretKey) -> Result<()> {
        std::fs::create_dir_all(&self.path)?;
        identity::store_for_test(&self.open_local().await?.store, identity)
    }

    /// Deletes a declined join: only the files a join makes (anything else
    /// in the folder stays, and the folder with it), then the device key. A
    /// key left by a crash in between opens nothing, and the next join works.
    async fn forget_unfinished(&self) -> Result<()> {
        // The store before the MLS database: a stop partway leaves an MLS
        // file without a store, which the next link clears again.
        for database in [STORE_FILE, MLS_FILE] {
            // Journal files first, so a stop partway never leaves one behind
            // for the next join's new database to find.
            for suffix in ["-wal", "-shm", ""] {
                remove_if_there(&self.path.join(format!("{database}{suffix}")))?;
            }
            #[cfg(test)]
            if CUT_SHORT_CLEARINGS
                .lock()
                .expect("cut-short clearings lock poisoned")
                .contains(&self.path)
            {
                bail!("a simulated stop while clearing");
            }
        }
        self.keys.forget(&self.path).await?;
        // Last, so whoever cleared still held it until here.
        remove_if_there(&self.path.join(LOCK_FILE))?;
        match std::fs::remove_dir(&self.path) {
            // Someone keeps other files there, which stay.
            Err(error)
                if !matches!(
                    error.kind(),
                    std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
                ) =>
            {
                return Err(anyhow::Error::new(error).context(format!("deleting {}", self.path.display())));
            }
            _ => {}
        }
        Ok(())
    }

    /// Clears what an unfinished join or link left in this folder, so a new
    /// link starts with a new key. A folder that is open elsewhere, holds a
    /// workspace (even one whose `workspace.json` is gone), or can't be read
    /// is refused, never cleared; a folder with only an identity keeps it.
    pub(super) async fn clear_unfinished_join(&self) -> Result<()> {
        if !self
            .path
            .try_exists()
            .with_context(|| format!("checking {}", self.path.display()))?
        {
            return Ok(());
        }
        // Checked before the lock too, so an open workspace is reported as
        // one rather than as a join in progress.
        if self.is_marked()? {
            return Ok(());
        }
        let lock = match self.lock() {
            Ok(lock) => lock,
            Err(error) => return Err(error.context("a join or link is in progress in this folder")),
        };
        if self.is_marked()? {
            return Ok(());
        }
        let has_store = self
            .store_path()
            .try_exists()
            .with_context(|| format!("checking {} for a store", self.path.display()))?;
        let has_mls = self
            .mls_path()
            .try_exists()
            .with_context(|| format!("checking {} for an MLS database", self.path.display()))?;
        if !has_mls {
            return Ok(());
        }
        if has_store {
            let refusal = || {
                format!(
                    "{} holds data that can't be read, so it isn't cleared",
                    self.path.display()
                )
            };
            let device_key = self.keys.load(&self.path).await.with_context(refusal)?;
            let store = Store::open(&self.store_path(), &device_key.database_key(STORE_FILE))
                .with_context(refusal)?;
            ensure!(
                !WorkspaceConfig::is_saved_in(&store)?,
                "{} holds a workspace whose workspace.json is missing; it isn't cleared",
                self.path.display()
            );
        }
        let cleared = self.forget_unfinished().await;
        drop(lock);
        cleared
    }

    fn is_marked(&self) -> Result<bool> {
        self.path
            .join(crate::config::CONFIG_FILE)
            .try_exists()
            .with_context(|| format!("checking {} for a workspace", self.path.display()))
    }

    /// This device's safety code, creating its identity if needed. A joiner
    /// shows it while waiting, so the admin can check who is asking.
    pub async fn safety_code(&self) -> Result<String> {
        std::fs::create_dir_all(&self.path).with_context(|| format!("creating {}", self.path.display()))?;
        Ok(safety_code(&self.open_local().await?.identity.public()))
    }

    pub(super) async fn open_local(&self) -> Result<Local> {
        let lock = self.lock()?;
        // A new key for a folder that already has its database would make
        // that data unreadable for good, so only a new folder gets one.
        let has_data = self
            .store_path()
            .try_exists()
            .with_context(|| format!("checking {} for a workspace's data", self.path.display()))?;
        let device_key = if has_data {
            self.keys.load(&self.path).await.with_context(|| {
                format!(
                    "this workspace's data is in {}, but its key is missing",
                    self.path.display()
                )
            })?
        } else {
            self.keys.load_or_create(&self.path).await?
        };
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
pub(super) struct Local {
    lock: std::fs::File,
    pub store: Store,
    pub identity: SecretKey,
    device_key: DeviceKey,
}

impl Local {
    pub(super) fn open_mls(&self, data: &DataDir, group_id: Option<&[u8]>) -> Result<Mls> {
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
        local.store.record_joined_epoch(mls.epoch()?)?;
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
        workspace.finish_writes_left_over()?;
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
            reads_signed_answers: true,
        };
        let asked = async {
            mailbox.post_request(seal_request(&invite, &request)?).await?;
            wait_for_answer(&invite, mailbox, cancel, &local.identity.public()).await
        }
        .await;
        let accepted = match asked {
            Ok(accepted) => accepted,
            // Declined (and signed so by the admin), the request is over:
            // asking again gets a new key, and with it a new safety code, which
            // the admin may need to see. A cancelled or expired one keeps its
            // key, as the admin may still approve it.
            Err(error) if error.downcast_ref::<Declined>().is_some() => {
                drop(mls);
                drop(local);
                clear_spent_join(data).await;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let facts = JoinFacts {
            workspace_id: &invite.workspace_id,
            admin: invite.admin,
            admin_mls_key: &invite.admin_mls_key,
        };
        // MLS deletes our key as it reads the Welcome, so a join that fails
        // from here on can't be finished from this folder.
        let granted = match admit(&local, &mut mls, &facts, accepted) {
            Ok(granted) => granted,
            Err(error) => {
                drop(mls);
                drop(local);
                clear_spent_join(data).await;
                return Err(error);
            }
        };
        let admitted = Admitted {
            config: WorkspaceConfig {
                workspace_id: invite.workspace_id,
                name: invite.workspace_name,
                admin: invite.admin,
                bucket: granted.bucket,
            },
            bucket_lamport: granted.bucket_lamport,
        };
        let pending = PendingProfile {
            name,
            is_bot: profile.is_bot,
        };
        Self::complete_join(data, local, mls, admitted, Some(pending), connect, options).await
    }

    /// Saves what the newcomer was admitted with, marks the workspace
    /// complete and starts it. A device linked to a person has no profile
    /// of its own to write.
    pub(super) async fn complete_join(
        data: &DataDir,
        local: Local,
        mls: Mls,
        admitted: Admitted,
        pending: Option<PendingProfile>,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Self> {
        let Admitted {
            config,
            bucket_lamport,
        } = admitted;
        config.save_in_store(&local.store)?;
        config::update_bucket(&local.store, &config.bucket, bucket_lamport)?;
        config::record_settings_op(&local.store, config::NO_SETTINGS_OP)?;
        if let Some(pending) = pending {
            local
                .store
                .set_meta(PENDING_PROFILE_META, &serde_json::to_vec(&pending)?)?;
        }
        config.mark_complete(&data.path)?;
        let workspace = Self::start(data, local, mls, config, connect, options).await?;
        // Without it, unbound commits are taken until the next bound one.
        if let Err(error) = workspace.note_a_bound_join().await {
            warn!("can't tell whether the admin's app binds its commits yet: {error:#}");
        }
        workspace.finish_writes_left_over()?;
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
        workspace.finish_writes_left_over()?;
        Ok(workspace)
    }

    /// Writes what opening owes (this device's sealing key, a name chosen when
    /// joining), unless a restore is still reading this device's log back:
    /// then catching up writes them.
    pub(crate) fn finish_writes_left_over(&self) -> Result<()> {
        if self.state().store.is_catching_up()? {
            return Ok(());
        }
        self.announce_sealing_key()?;
        self.finish_pending_profile()
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
        state.record_epoch(&config)?;
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
        // A device's person is set before it first starts, and never changes.
        let person = state.store.person_of(&identity.public())?;
        let workspace = Self {
            inner: Arc::new(Inner {
                data: data.clone(),
                _data_lock: lock,
                device_key,
                identity,
                person,
                huddle_media: options.media.clone(),
                device_settings: options.device_settings.clone(),
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
                page_live: Mutex::default(),
                page_alert_times: Mutex::default(),
                #[cfg(test)]
                page_sent: Mutex::default(),
                last_slow_duties: Mutex::new(None),
                verified_commits: Mutex::new(std::collections::HashMap::new()),
                removal_reads: Mutex::new(std::collections::HashMap::new()),
                seen_heads: Mutex::new(std::collections::HashMap::new()),
                confirmed_head_seq: Default::default(),
                heard_hints: Mutex::new(std::collections::HashMap::new()),
                relays: Mutex::new(Default::default()),
                key_round: Mutex::new(Default::default()),
                key_answers: Mutex::new(std::collections::HashMap::new()),
                refused_requests: Mutex::default(),
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
                audience_lock: Mutex::default(),
                #[cfg(test)]
                audience_pause: Mutex::default(),
                #[cfg(test)]
                drop_segment_transaction: Default::default(),
                #[cfg(test)]
                fail_next_answer_upload: Default::default(),
                #[cfg(test)]
                fail_next_commit_upload: Default::default(),
                #[cfg(test)]
                tamper_next_grant: Default::default(),
                #[cfg(test)]
                refuse_next_sealed_write: Default::default(),
            }),
        };
        workspace.rebuild_views_if_rules_changed()?;
        // A removal settled before a crash may still owe the views their share.
        if let Err(error) = workspace.finish_view_work() {
            warn!("the views will catch up with a removal at the next pass: {error:#}");
        }
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
    me: &PublicKey,
) -> Result<Accepted> {
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
                // The invite's admin signs every acceptance: an unsigned
                // one is someone with the bucket keys trying to spend our key.
                Answer::Accepted { .. } if invite.signs_acceptance => {
                    warn!("ignoring an answer the admin didn't sign");
                }
                Answer::Accepted {
                    welcome,
                    sealed_grant,
                } => {
                    return Ok(Accepted {
                        welcome,
                        sealed_grant,
                        map_digest: None,
                    });
                }
                Answer::AcceptedSigned {
                    welcome,
                    sealed_grant,
                    map_digest,
                    signature,
                } => {
                    let acceptance = Accepted {
                        welcome,
                        sealed_grant,
                        map_digest,
                    };
                    if is_signed_acceptance(invite, me, &acceptance, &signature) {
                        return Ok(acceptance);
                    }
                    warn!("ignoring an answer the admin didn't sign");
                }
                Answer::Rejected { signature } => {
                    if is_signed_decline(invite, me, &signature) {
                        return Err(Declined.into());
                    }
                    warn!("ignoring a decline the admin didn't sign");
                }
            }
        }
        ensure!(
            now_ms()? < invite.expires_ms,
            "the invite expired before the admin answered; ask them for a new one"
        );
        tokio::select! {
            () = tokio::time::sleep(ANSWER_POLL) => {}
            _ = cancelled.wait_for(|is_cancelled| *is_cancelled) => bail!("you cancelled the join request"),
        }
    }
}

/// The admin's acceptance of our join request.
pub(super) struct Accepted {
    pub welcome: Vec<u8>,
    pub sealed_grant: Vec<u8>,
    /// The device map's digest, once the workspace has a linked device:
    /// the Welcome needs it as a pre-shared key.
    pub map_digest: Option<[u8; 32]>,
}

/// Where the workspace's bucket is, from the grant.
pub(super) struct GrantedBucket {
    pub bucket: BucketConfig,
    pub bucket_lamport: u64,
}

/// What a newcomer was let in with, ready to save.
pub(super) struct Admitted {
    pub config: WorkspaceConfig,
    pub bucket_lamport: u64,
}

/// The workspace facts a newcomer joins with, from its invite or link.
pub(super) struct JoinFacts<'a> {
    pub workspace_id: &'a str,
    pub admin: PublicKey,
    pub admin_mls_key: &'a [u8],
}

/// Joins the group with the admin's answer and stores the grant it seals.
/// A grant whose device map doesn't match the answer's digest is refused
/// before anything is stored.
pub(super) fn admit(
    local: &Local,
    mls: &mut Mls,
    facts: &JoinFacts,
    accepted: Accepted,
) -> Result<GrantedBucket> {
    if let Some(map_digest) = &accepted.map_digest {
        mls.store_map_key(map_digest)?;
    }
    mls.join(
        &accepted.welcome,
        facts.workspace_id.as_bytes(),
        &facts.admin,
        facts.admin_mls_key,
    )?;
    let opened = mls.open_from_admin(&accepted.sealed_grant, &facts.admin)?;
    let context = ApplyContext {
        workspace_id: facts.workspace_id,
        admin: facts.admin,
        me: local.identity.public(),
    };
    let granted = match accepted.map_digest {
        None => {
            let Grant {
                history,
                bucket,
                bucket_lamport,
            } = postcard::from_bytes(&opened)?;
            history.store_into(&local.store, context)?;
            GrantedBucket {
                bucket,
                bucket_lamport,
            }
        }
        Some(expected) => {
            let with_devices: GrantWithDevices = postcard::from_bytes(&opened)?;
            // Before anything is stored, written or announced: a map the
            // members don't share would split the workspace.
            check_device_map(facts.workspace_id, &with_devices, &expected)?;
            let GrantWithDevices {
                grant:
                    Grant {
                        history,
                        bucket,
                        bucket_lamport,
                    },
                devices,
                bound_frozen,
            } = with_devices;
            history.store_with_devices(&devices, &bound_frozen, &local.store, context)?;
            GrantedBucket {
                bucket,
                bucket_lamport,
            }
        }
    };
    local.store.record_joined_epoch(mls.epoch()?)?;
    Ok(granted)
}

/// Checks that the grant's device map and bound freezes are the ones the
/// admin committed with, which every member also used, and that every
/// record carries both of its signatures.
fn check_device_map(workspace_id: &str, granted: &GrantWithDevices, expected: &[u8; 32]) -> Result<()> {
    let is_signed = granted
        .devices
        .iter()
        .all(|record| record.certificate.verify(workspace_id).is_ok());
    let digest = map_digest(workspace_id, &granted.devices, &granted.bound_frozen)?;
    ensure!(
        is_signed && digest == *expected,
        "the list of devices the admin sent doesn't match the one the workspace uses, so this join can't \
         finish; ask the admin for a new invite"
    );
    Ok(())
}

/// Forgets a join that can't go on: its key is spent, and the next try
/// makes a new one.
pub(super) async fn clear_spent_join(data: &DataDir) {
    if let Err(cleanup) = data.forget_unfinished().await {
        warn!(
            "couldn't clear a join that ended from {}: {cleanup:#}",
            data.path.display()
        );
    }
}

/// The admin declined the join request, as their signature shows.
#[derive(Debug)]
struct Declined;

impl std::fmt::Display for Declined {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the admin declined this request")
    }
}

impl std::error::Error for Declined {}

/// Whether the invite's admin signed this decline of our request; anyone
/// with the bucket keys can write the answer slot.
fn is_signed_decline(invite: &Invite, me: &PublicKey, signature: &[u8]) -> bool {
    is_signed_by_admin(
        invite,
        declined_bytes(&invite.workspace_id, &invite.token, me),
        signature,
    )
}

/// Whether the invite's admin signed this acceptance, all of it. Checked
/// before MLS sees the Welcome, which it would use up.
fn is_signed_acceptance(invite: &Invite, me: &PublicKey, acceptance: &Accepted, signature: &[u8]) -> bool {
    let bytes = accepted_bytes(
        &invite.workspace_id,
        &invite.token,
        me,
        acceptance.map_digest.as_ref(),
        &acceptance.welcome,
        &acceptance.sealed_grant,
    );
    is_signed_by_admin(invite, bytes, signature)
}

fn is_signed_by_admin(invite: &Invite, bytes: Result<Vec<u8>>, signature: &[u8]) -> bool {
    let Ok(bytes) = bytes else {
        return false;
    };
    let Ok(signature) = <[u8; 64]>::try_from(signature) else {
        return false;
    };
    invite
        .admin
        .verify(&bytes, &iroh::Signature::from_bytes(&signature))
        .is_ok()
}

/// Folders whose create stops just after it is complete, as if the app
/// died then; keyed by folder so tests running alongside aren't touched.
#[cfg(test)]
pub(crate) static CUT_SHORT_CREATES: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

/// Folders whose clearing stops after its first database, as a crash or a
/// file that won't delete would stop it.
#[cfg(test)]
pub(crate) static CUT_SHORT_CLEARINGS: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

#[derive(Serialize, Deserialize)]
pub(super) struct PendingProfile {
    name: String,
    is_bot: bool,
}

fn remove_if_there(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(anyhow::Error::new(error).context(format!("deleting {}", path.display())))
        }
        _ => Ok(()),
    }
}

pub(super) fn prepare_empty_dir(dir: &Path) -> Result<()> {
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
