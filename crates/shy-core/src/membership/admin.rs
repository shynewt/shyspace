//! The admin's duties: letting people in, removing them, and keeping the
//! bucket's copy of every commit intact.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::{commit::SignedCommit, invite::Invite, keyring};
use crate::{
    storage::Store,
    views::{Change, parse_member_id},
    workspace::{State, Workspace},
};

/// What the admin still owes a joiner: their Welcome and the key history.
/// Saved before the commit is applied and kept until the joiner posts, so
/// neither a crash nor a failed upload strands them.
#[derive(Serialize, Deserialize)]
struct Delivery {
    welcome: Vec<u8>,
    /// Sealed right after the commit is applied; empty until then.
    keyring: Option<(u64, Vec<u8>)>,
}

/// After a crash before a join commit was recorded, its Welcome is useless.
pub(crate) fn drop_unsealed_deliveries(store: &Store) -> Result<()> {
    for (joiner, payload) in store.deliveries()? {
        if postcard::from_bytes::<Delivery>(&payload)?.keyring.is_none() {
            store.delete_delivery(&joiner)?;
        }
    }
    Ok(())
}

impl Workspace {
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

    pub(crate) async fn admin_duties(&self) -> Result<()> {
        self.republish_commits().await?;
        self.accept_join_requests().await?;
        self.deliver_welcomes().await
    }

    async fn accept_join_requests(&self) -> Result<()> {
        let token = self
            .inner
            .config
            .invite_token
            .clone()
            .context("admin has no invite token")?;
        for (path, request) in self.inner.bucket.join_requests(&token).await? {
            let verdict = request
                .context("malformed join request")
                .and_then(|(joiner, request)| {
                    let mut state = self.state();
                    let (claimed, key_package) = state
                        .mls
                        .verify_key_package(&request.key_package, &request.binding)?;
                    ensure!(claimed == joiner, "join request filed under someone else's id");
                    if state.mls.is_member(&joiner)? {
                        return Ok(None);
                    }
                    let from_epoch = state.mls.epoch()?;
                    let (commit, welcome) = state.mls.add_member(key_package)?;
                    let delivery = Delivery {
                        welcome,
                        keyring: None,
                    };
                    state
                        .store
                        .put_delivery(&joiner, &postcard::to_stdvec(&delivery)?)?;
                    let signed = self.record_commit(&state, from_epoch, commit)?;
                    state.mls.merge_pending_commit()?;
                    state.record_epoch()?;
                    let delivery = Delivery {
                        keyring: Some(keyring::seal(&mut state)?),
                        ..delivery
                    };
                    state
                        .store
                        .put_delivery(&joiner, &postcard::to_stdvec(&delivery)?)?;
                    Ok(Some((from_epoch, signed)))
                });
            match verdict {
                Ok(Some((from_epoch, signed))) => {
                    self.emit(Change::Members);
                    self.upload_commit(from_epoch, signed).await?;
                }
                Ok(None) => {}
                Err(error) => warn!("rejecting join request {path}: {error:#}"),
            }
            self.inner.bucket.delete(&path).await?;
        }
        Ok(())
    }

    /// Uploads owed Welcomes, key history first (a joiner reads it right
    /// after the Welcome appears). Re-sent every pass until the joiner posts
    /// their first op, which also repairs copies overwritten in the bucket.
    async fn deliver_welcomes(&self) -> Result<()> {
        let deliveries = self.state().store.deliveries()?;
        for (joiner, payload) in deliveries {
            if self.state().store.has_ops_from(&joiner)? {
                self.state().store.delete_delivery(&joiner)?;
                continue;
            }
            let delivery: Delivery = postcard::from_bytes(&payload)?;
            let Some((epoch, sealed_keyring)) = delivery.keyring else {
                warn!("no key history sealed for {joiner}; they need a new invite");
                self.state().store.delete_delivery(&joiner)?;
                continue;
            };
            self.inner.bucket.put_keyring(epoch, sealed_keyring).await?;
            self.inner.bucket.put_welcome(&joiner, delivery.welcome).await?;
        }
        Ok(())
    }

    pub async fn remove_member(&self, member_id: &str) -> Result<()> {
        let member = parse_member_id(member_id)?;
        ensure!(self.is_admin(), "only the workspace admin can remove members");
        ensure!(member != self.me(), "the admin cannot remove themselves");
        let (from_epoch, signed) = {
            let mut state = self.state();
            let from_epoch = state.mls.epoch()?;
            let commit = state.mls.remove_member(&member)?;
            let signed = self.record_commit(&state, from_epoch, commit)?;
            state.mls.merge_pending_commit()?;
            state.record_epoch()?;
            (from_epoch, signed)
        };
        self.emit(Change::Members);
        self.upload_commit(from_epoch, signed).await
    }

    /// Signs a staged commit and saves it before it is applied, so a crash
    /// can't leave us in an epoch the bucket never hears about.
    fn record_commit(&self, state: &State, from_epoch: u64, commit: Vec<u8>) -> Result<Vec<u8>> {
        let signed = SignedCommit::sign(&self.inner.identity, from_epoch, commit)?;
        state.store.put_commit_record(from_epoch, &signed)?;
        Ok(signed)
    }

    async fn upload_commit(&self, from_epoch: u64, signed: Vec<u8>) -> Result<()> {
        self.inner.bucket.put_commit(from_epoch, signed).await?;
        self.nudge_peers().await;
        Ok(())
    }

    /// Re-uploads any of our commits that are missing or altered in the
    /// bucket (a failed upload, or tampering by someone with bucket access).
    /// There is one commit per membership change, so this stays cheap.
    async fn republish_commits(&self) -> Result<()> {
        let records = self.state().store.commit_records()?;
        for (from_epoch, signed) in records {
            if self.inner.bucket.commit(from_epoch).await?.as_deref() != Some(signed.as_slice()) {
                self.inner.bucket.put_commit(from_epoch, signed).await?;
            }
        }
        Ok(())
    }
}
