//! Creating, joining and reopening a workspace.

use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use iroh::SecretKey;
use tokio::sync::{Notify, broadcast, mpsc};

use super::{Inner, Networking, Profile, State, Workspace};
use crate::{
    config::WorkspaceConfig,
    identity,
    membership::{
        invite::{Answer, Grant, Invite, JoinMailbox, JoinRequest, ensure_not_expired},
        mls::{Mls, binding_message},
    },
    ops::{Op, new_owned_id, new_random_id, now_ms},
    pages::Edit,
    storage::Store,
    sync::SyncStatus,
    transport::{
        bucket::{Bucket, BucketConfig},
        gossip::Net,
        presigned::PresignedMailbox,
    },
};

const STORE_FILE: &str = "shy.db";
const MLS_FILE: &str = "mls.db";
const SIGNER_META: &str = "mls_signer_public";
const ANSWER_POLL: Duration = Duration::from_secs(2);

const GETTING_STARTED: &str = "# Welcome to shyspace\n\n\
Everything here is end-to-end encrypted. Your bucket only ever stores ciphertext.\n\n\
- **Chat** in channels, react, and reply.\n\
- **Pages** are collaborative markdown. Type together in real time.\n\
- **Share a page** in chat by pasting its link; the card stays live.\n\
- **Agents** join with an invite code like anyone else.\n\n\
Press `Ctrl K` to search everything.\n";

impl Workspace {
    pub fn exists(dir: &Path) -> bool {
        WorkspaceConfig::exists(dir)
    }

    pub async fn open(dir: &Path, networking: Networking) -> Result<Self> {
        let config = WorkspaceConfig::load(dir)?;
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

    /// Asks the admin to let us in and waits until they decide or the invite
    /// expires. The admin approves from their app.
    pub async fn join(
        dir: &Path,
        invite_code: &str,
        profile: Profile<'_>,
        networking: Networking,
    ) -> Result<Self> {
        let invite = Invite::decode(invite_code)?;
        let mailbox = PresignedMailbox::for_invite(&invite);
        let workspace_id = invite.workspace_id.clone();
        Self::join_with(
            dir,
            invite,
            profile,
            &mailbox,
            |bucket| bucket.connect(&workspace_id),
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
        prepare_empty_dir(dir)?;
        let identity = identity::load_or_create(dir)?;
        let store = Store::open(&dir.join(STORE_FILE))?;
        let mut mls = Mls::open(&dir.join(MLS_FILE), &identity, None, None)?;
        mls.create_group(workspace_id.as_bytes())?;
        store.set_meta(SIGNER_META, mls.signer_public())?;
        let config = WorkspaceConfig {
            workspace_id,
            name: workspace_name.to_owned(),
            admin: identity.public(),
            bucket: bucket_config,
        };
        config.save(dir)?;
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

    pub(crate) async fn join_with(
        dir: &Path,
        invite: Invite,
        profile: Profile<'_>,
        mailbox: &impl JoinMailbox,
        connect: impl FnOnce(&BucketConfig) -> Result<Bucket>,
        networking: Networking,
    ) -> Result<Self> {
        ensure_not_expired(&invite, now_ms()?)?;
        prepare_empty_dir(dir)?;
        let identity = identity::load_or_create(dir)?;
        let store = Store::open(&dir.join(STORE_FILE))?;
        let signer = store.meta(SIGNER_META)?;
        let mut mls = Mls::open(&dir.join(MLS_FILE), &identity, signer.as_deref(), None)?;
        store.set_meta(SIGNER_META, mls.signer_public())?;

        let request = JoinRequest {
            key_package: mls.key_package()?,
            binding: identity
                .sign(&binding_message(mls.signer_public()))
                .to_bytes()
                .to_vec(),
            name: profile.name.trim().to_owned(),
            is_bot: profile.is_bot,
        };
        mailbox.post_request(postcard::to_stdvec(&request)?).await?;
        let (welcome, sealed_grant) = wait_for_answer(&invite, mailbox).await?;
        mls.join(
            &welcome,
            invite.workspace_id.as_bytes(),
            &invite.admin,
            &invite.admin_mls_key,
        )?;
        let grant: Grant = postcard::from_bytes(&mls.open_from_admin(&sealed_grant, &invite.admin)?)?;
        grant.history.store_into(&store)?;

        let bucket = connect(&grant.bucket)?;
        let config = WorkspaceConfig {
            workspace_id: invite.workspace_id,
            name: invite.workspace_name,
            admin: invite.admin,
            bucket: grant.bucket,
        };
        config.save(dir)?;
        let workspace = Self::start(identity, config, State { store, mls }, bucket, networking).await?;
        workspace.write(Op::Profile {
            name: profile.name.to_owned(),
            is_bot: profile.is_bot,
        })?;
        Ok(workspace)
    }

    pub(crate) async fn open_with(dir: &Path, bucket: Bucket, networking: Networking) -> Result<Self> {
        let config = WorkspaceConfig::load(dir)?;
        let identity = identity::load_or_create(dir)?;
        let store = Store::open(&dir.join(STORE_FILE))?;
        let signer = store
            .meta(SIGNER_META)?
            .context("MLS signing key reference missing")?;
        let mls = Mls::open(
            &dir.join(MLS_FILE),
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
        if let Some(net) = &workspace.inner.net {
            net.set_members(&workspace.current_members()?);
            workspace.spawn_background(incoming);
        }
        Ok(workspace)
    }
}

/// Polls for the admin's answer until the invite expires.
async fn wait_for_answer(invite: &Invite, mailbox: &impl JoinMailbox) -> Result<(Vec<u8>, Vec<u8>)> {
    loop {
        // Undecodable bytes are junk from someone with bucket access; the
        // admin re-sends the real answer, so keep polling.
        if let Some(bytes) = mailbox.fetch_answer().await?
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
        tokio::time::sleep(ANSWER_POLL).await;
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
