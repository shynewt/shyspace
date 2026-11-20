//! The admin's duties: inviting people, deciding on join requests, removing
//! members, and keeping the bucket's copy of every commit intact.

use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::{
    commit::{self, FrozenAuthor},
    invite::{Answer, Grant, Invite, JoinRequest},
    keyring::KeyHistory,
};
use crate::{
    config,
    ops::{new_random_id, now_ms},
    storage::Store,
    transport::bucket::InviteLinks,
    views::{self, Change, InviteState, parse_member_id},
    workspace::{State, Workspace},
};

/// Presigned S3 links can't outlive seven days.
const INVITE_LIFETIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_REQUESTER_NAME: usize = 80;

/// What the admin still owes a joiner. Saved before the commit is applied and
/// kept until the joiner posts, so neither a crash nor a failed upload strands
/// them; re-uploading also repairs an answer someone overwrote.
#[derive(Serialize, Deserialize)]
struct Delivery {
    welcome: Vec<u8>,
    /// Sealed right after the commit is applied; empty until then.
    sealed_grant: Option<Vec<u8>>,
}

/// After a crash before a join commit was recorded, its Welcome is useless.
pub(crate) fn drop_unsealed_deliveries(store: &Store) -> Result<()> {
    for (joiner, _, payload) in store.deliveries()? {
        if postcard::from_bytes::<Delivery>(&payload)?.sealed_grant.is_none() {
            store.delete_delivery(&joiner)?;
        }
    }
    Ok(())
}

impl Workspace {
    fn ensure_admin(&self) -> Result<()> {
        ensure!(self.is_admin(), "only the workspace admin can manage members");
        Ok(())
    }

    /// Creates a single-use invite and returns its code. `label` is a note
    /// for the admin, like who it's for.
    pub async fn create_invite(&self, label: &str) -> Result<String> {
        self.ensure_admin()?;
        let token = new_random_id();
        let links = self.bucket().presign_invite(&token, INVITE_LIFETIME).await?;
        self.record_invite(token, label, INVITE_LIFETIME, links)?.encode()
    }

    fn record_invite(
        &self,
        token: String,
        label: &str,
        lifetime: Duration,
        links: InviteLinks,
    ) -> Result<Invite> {
        let created_ms = now_ms()?;
        let expires_ms = created_ms + i64::try_from(lifetime.as_millis())?;
        let config = &self.inner.config;
        let invite = Invite {
            workspace_id: config.workspace_id.clone(),
            workspace_name: config.name.clone(),
            admin: config.admin,
            admin_mls_key: self.state().mls.signer_public().to_vec(),
            token: token.clone(),
            expires_ms,
            request_url: links.request_url,
            answer_url: links.answer_url,
        };
        self.state()
            .store
            .insert_invite(&token, label.trim(), created_ms, expires_ms)?;
        self.emit(Change::Invites);
        Ok(invite)
    }

    /// An invite without presigned links, since in-memory buckets can't sign.
    #[cfg(test)]
    pub(crate) fn create_test_invite(&self, label: &str, lifetime: Duration) -> Result<Invite> {
        self.ensure_admin()?;
        let links = InviteLinks {
            request_url: String::new(),
            answer_url: String::new(),
        };
        self.record_invite(new_random_id(), label, lifetime, links)
    }

    pub fn invites(&self) -> Result<Vec<views::Invite>> {
        self.ensure_admin()?;
        self.state().store.invites(now_ms()?)
    }

    /// Lets the requester in: adds them to the group and answers with the
    /// Welcome, the key history and the bucket credentials.
    pub async fn approve_invite(&self, token: &str) -> Result<()> {
        self.ensure_admin()?;
        let (from_epoch, signed) = {
            let mut state = self.state();
            let pending = state
                .store
                .pending_request(token, now_ms()?)?
                .context("this invite has no request waiting for approval")?;
            let request: JoinRequest = postcard::from_bytes(&pending.request)?;
            let (claimed, key_package) = state
                .mls
                .verify_key_package(&request.key_package, &request.binding)?;
            ensure!(
                claimed == pending.joiner,
                "the stored request doesn't match its requester"
            );
            ensure!(!state.mls.is_member(&claimed)?, "this person is already a member");

            let from_epoch = state.mls.epoch()?;
            let (commit, welcome) = state.mls.add_member(key_package)?;
            let delivery = Delivery {
                welcome,
                sealed_grant: None,
            };
            state
                .store
                .put_delivery(&claimed, token, &postcard::to_stdvec(&delivery)?)?;
            let signed = self.record_commit(&state, from_epoch, commit, Vec::new())?;
            state.mls.merge_pending_commit()?;
            state.record_epoch()?;
            let grant = Grant {
                history: KeyHistory::collect(&state.store)?,
                bucket: config::current_bucket(&state.store)?,
                bucket_lamport: config::bucket_lamport(&state.store)?,
            };
            let sealed_grant = state.mls.seal_for_members(&postcard::to_stdvec(&grant)?)?;
            let delivery = Delivery {
                sealed_grant: Some(sealed_grant),
                ..delivery
            };
            state
                .store
                .put_delivery(&claimed, token, &postcard::to_stdvec(&delivery)?)?;
            state
                .store
                .change_invite_state(token, &[InviteState::Requested], InviteState::Accepted)?;
            Workspace::mark_recovery_snapshot_stale(&state.store)?;
            (from_epoch, signed)
        };
        self.emit(Change::Members);
        self.emit(Change::Invites);
        // Snapshot first: a commit must never be in the bucket before the
        // recovery snapshot that includes it.
        self.refresh_recovery_snapshot().await?;
        self.upload_commit(from_epoch, signed).await?;
        self.deliver_answers().await?;
        self.bucket().delete_invite_request(token).await
    }

    pub async fn reject_invite(&self, token: &str) -> Result<()> {
        self.ensure_admin()?;
        let changed = self.state().store.change_invite_state(
            token,
            &[InviteState::Requested],
            InviteState::Rejected,
        )?;
        ensure!(changed, "this invite has no request waiting for a decision");
        self.emit(Change::Invites);
        self.bucket()
            .put_invite_answer(token, postcard::to_stdvec(&Answer::Rejected)?)
            .await?;
        self.bucket().delete_invite_request(token).await
    }

    /// Stops an invite from being used. Its links stay valid until they
    /// expire, but anything posted through them is ignored.
    pub async fn revoke_invite(&self, token: &str) -> Result<()> {
        self.ensure_admin()?;
        let changed = self.state().store.change_invite_state(
            token,
            &[InviteState::Open, InviteState::Requested],
            InviteState::Revoked,
        )?;
        ensure!(changed, "this invite is already used, revoked or unknown");
        self.emit(Change::Invites);
        self.bucket().delete_invite_request(token).await
    }

    pub(crate) async fn admin_duties(&self) -> Result<()> {
        // The snapshot goes first so commits are only ever (re)published
        // once the recovery snapshot covers them.
        self.refresh_recovery_snapshot().await?;
        self.republish_commits().await?;
        self.collect_join_requests().await?;
        self.deliver_answers().await
    }

    /// Picks up requests posted through open invites, for the admin to decide.
    async fn collect_join_requests(&self) -> Result<()> {
        let tokens = self.state().store.open_invite_tokens(now_ms()?)?;
        for token in tokens {
            let Some(bytes) = self.bucket().invite_request(&token).await? else {
                continue;
            };
            match self.record_request(&token, &bytes) {
                Ok(()) => self.emit(Change::Invites),
                Err(error) => {
                    // Drop it so a genuine request can take the slot.
                    warn!("ignoring an invalid join request: {error:#}");
                    self.bucket().delete_invite_request(&token).await?;
                }
            }
        }
        Ok(())
    }

    fn record_request(&self, token: &str, bytes: &[u8]) -> Result<()> {
        let request: JoinRequest = postcard::from_bytes(bytes)?;
        let state = self.state();
        let (joiner, _) = state
            .mls
            .verify_key_package(&request.key_package, &request.binding)?;
        let name: String = request.name.trim().chars().take(MAX_REQUESTER_NAME).collect();
        ensure!(!name.is_empty(), "the request has no name");
        state
            .store
            .record_join_request(token, &joiner, &name, request.is_bot, bytes)
    }

    /// Uploads owed answers until each joiner posts their first op, then
    /// clears the invite's slots in the bucket.
    async fn deliver_answers(&self) -> Result<()> {
        let deliveries = self.state().store.deliveries()?;
        for (joiner, token, payload) in deliveries {
            if self.state().store.has_ops_from(&joiner)? {
                self.bucket().delete_invite_answer(&token).await?;
                self.bucket().delete_invite_request(&token).await?;
                self.state().store.delete_delivery(&joiner)?;
                continue;
            }
            let delivery: Delivery = postcard::from_bytes(&payload)?;
            let Some(sealed_grant) = delivery.sealed_grant else {
                warn!("no grant sealed for {joiner}; they need a new invite");
                self.state().store.delete_delivery(&joiner)?;
                continue;
            };
            let answer = Answer::Accepted {
                welcome: delivery.welcome,
                sealed_grant,
            };
            self.bucket()
                .put_invite_answer(&token, postcard::to_stdvec(&answer)?)
                .await?;
        }
        Ok(())
    }

    /// Removes a member. Their log is frozen at the last op we have from
    /// them, so we first pull everything they wrote.
    pub async fn remove_member(&self, member_id: &str) -> Result<()> {
        self.ensure_admin()?;
        let member = parse_member_id(member_id)?;
        if member == self.me() {
            bail!("the admin cannot remove themselves");
        }
        self.pull()
            .await
            .context("can't reach the bucket to collect the member's last messages")?;
        let (from_epoch, signed) = {
            let mut state = self.state();
            let frozen = FrozenAuthor {
                member,
                last_seq: state.store.last_seq(&member)?,
            };
            let from_epoch = state.mls.epoch()?;
            let commit = state.mls.remove_member(&member)?;
            state
                .store
                .freeze_author(&frozen.member, frozen.last_seq, &self.me())?;
            let signed = self.record_commit(&state, from_epoch, commit, vec![frozen])?;
            state.mls.merge_pending_commit()?;
            state.record_epoch()?;
            Workspace::mark_recovery_snapshot_stale(&state.store)?;
            (from_epoch, signed)
        };
        self.emit(Change::Members);
        self.refresh_recovery_snapshot().await?;
        self.upload_commit(from_epoch, signed).await
    }

    /// Signs a staged commit and saves it before it is applied, so a crash
    /// can't leave us in an epoch the bucket never hears about.
    fn record_commit(
        &self,
        state: &State,
        from_epoch: u64,
        commit: Vec<u8>,
        frozen: Vec<FrozenAuthor>,
    ) -> Result<Vec<u8>> {
        let signed = commit::sign(&self.inner.identity, from_epoch, commit, frozen)?;
        state.store.put_commit_record(from_epoch, &signed)?;
        Ok(signed)
    }

    async fn upload_commit(&self, from_epoch: u64, signed: Vec<u8>) -> Result<()> {
        self.bucket().put_commit(from_epoch, signed).await?;
        self.nudge_peers().await;
        Ok(())
    }
}
