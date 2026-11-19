//! Background sync with the bucket, plus the admin's membership duties.

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    crypto::{self, EpochKey},
    net::Wire,
    ops::{Envelope, VerifiedOp},
    views::Change,
    workspace::{State, Workspace, now_ms, parse_member_id},
};

const SYNC_INTERVAL: Duration = Duration::from_secs(8);
/// Batches bursts of writes (like typing) into one bucket upload.
const PUSH_DEBOUNCE: Duration = Duration::from_millis(600);
/// Ops for epochs further ahead than this are dropped instead of parked, so
/// junk can't pile up waiting for a key that will never come.
const MAX_PENDING_EPOCHS_AHEAD: u64 = 16;
/// SQLite integers are signed 64-bit.
const MAX_STORED_NUMBER: u64 = i64::MAX as u64;
const COMMIT_LABEL: &[u8] = b"shyspace commit v1";

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    pub peers: usize,
    pub last_sync_ms: Option<i64>,
    pub error: Option<String>,
    pub removed: bool,
}

/// One epoch of history handed to newcomers: its key and who could write in it.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct KeyringEntry {
    pub epoch: u64,
    pub key: EpochKey,
    pub members: Vec<PublicKey>,
}

/// An MLS commit wrapped in the admin's identity signature. Readers ignore
/// anything else in a commit slot, so bucket junk can't wedge the chain.
#[derive(Debug, Serialize, Deserialize)]
struct SignedCommit {
    from_epoch: u64,
    commit: Vec<u8>,
    signature: Vec<u8>,
}

fn commit_message(from_epoch: u64, commit: &[u8]) -> Vec<u8> {
    [COMMIT_LABEL, &from_epoch.to_be_bytes(), commit].concat()
}

impl SignedCommit {
    fn verify(bytes: &[u8], from_epoch: u64, admin: &PublicKey) -> Result<Vec<u8>> {
        let signed: SignedCommit = postcard::from_bytes(bytes)?;
        ensure!(signed.from_epoch == from_epoch, "commit is for another epoch");
        let signature: [u8; 64] = signed
            .signature
            .as_slice()
            .try_into()
            .context("malformed signature")?;
        admin
            .verify(
                &commit_message(from_epoch, &signed.commit),
                &Signature::from_bytes(&signature),
            )
            .context("commit not signed by the admin")?;
        Ok(signed.commit)
    }
}

/// What the admin still owes a joiner: their Welcome and the key history.
/// Saved before the commit is applied and kept until uploaded, so neither a
/// crash nor a failed upload strands the joiner.
#[derive(Serialize, Deserialize)]
struct Delivery {
    welcome: Vec<u8>,
    /// Sealed right after the commit is applied; empty until then.
    keyring: Option<(u64, Vec<u8>)>,
}

pub(crate) fn drop_unsealed_deliveries(store: &crate::store::Store) -> Result<()> {
    for (joiner, payload) in store.deliveries()? {
        if postcard::from_bytes::<Delivery>(&payload)?.keyring.is_none() {
            store.delete_delivery(&joiner)?;
        }
    }
    Ok(())
}

fn seal_keyring(state: &mut State) -> Result<(u64, Vec<u8>)> {
    let mut keyring = Vec::new();
    for (epoch, key) in state.store.epoch_keys()? {
        keyring.push(KeyringEntry {
            epoch,
            key,
            members: state.store.epoch_members(epoch)?,
        });
    }
    let epoch = state.mls.epoch()?;
    Ok((
        epoch,
        state.mls.seal_for_members(&postcard::to_stdvec(&keyring)?)?,
    ))
}

impl Workspace {
    pub(crate) fn spawn_background(&self, mut incoming: mpsc::Receiver<Wire>) {
        let receiver = self.clone();
        tokio::spawn(async move {
            while let Some(wire) = incoming.recv().await {
                match wire {
                    Wire::Envelope(envelope) => {
                        if let Err(error) = receiver.receive(envelope) {
                            warn!("dropping live op: {error:#}");
                        }
                    }
                    Wire::Nudge(_) => receiver.inner.wake.notify_one(),
                }
            }
        });

        let syncer = self.clone();
        tokio::spawn(async move {
            if let Some(net) = &syncer.inner.net {
                let me = syncer.me();
                if let Err(error) = syncer.inner.bucket.announce_peer(&me).await {
                    warn!("could not announce ourselves: {error}");
                }
                let _ = net.nudge().await;
            }
            loop {
                syncer.sync().await;
                if syncer.status().removed {
                    break;
                }
                tokio::select! {
                    _ = syncer.inner.wake.notified() => tokio::time::sleep(PUSH_DEBOUNCE).await,
                    _ = tokio::time::sleep(SYNC_INTERVAL) => {}
                }
            }
        });
    }

    pub fn status(&self) -> SyncStatus {
        let mut status = self.inner.status.lock().expect("status lock poisoned").clone();
        status.peers = self.inner.net.as_ref().map_or(0, |net| net.neighbors());
        status
    }

    /// One full pass. Each step runs even if an earlier one failed, so one
    /// bad object can't stop the others; the first error is shown in the UI.
    pub async fn sync(&self) {
        let _guard = self.inner.sync_lock.lock().await;
        let mut errors = Vec::new();
        if let Err(error) = self.advance_epochs().await {
            errors.push(error);
        }
        if let Err(error) = self.apply_pending() {
            errors.push(error);
        }
        if !self.status().removed {
            if self.is_admin()
                && let Err(error) = self.admin_duties().await
            {
                errors.push(error);
            }
            for result in [self.pull().await, self.push().await, self.join_peers().await] {
                if let Err(error) = result {
                    errors.push(error);
                }
            }
        }
        {
            let mut status = self.inner.status.lock().expect("status lock poisoned");
            match errors.first() {
                None => {
                    status.last_sync_ms = Some(now_ms());
                    status.error = None;
                }
                Some(error) => status.error = Some(format!("{error:#}")),
            }
        }
        self.emit(Change::Sync);
    }

    async fn join_peers(&self) -> Result<()> {
        if let Some(net) = &self.inner.net {
            net.join(self.inner.bucket.peers().await?).await?;
        }
        Ok(())
    }

    /// Applies admin commits in epoch order until we are up to date.
    async fn advance_epochs(&self) -> Result<()> {
        let admin = self.inner.config.admin;
        if self.me() == admin {
            return Ok(());
        }
        loop {
            let epoch = {
                let state = self.state();
                if !state.mls.is_active() {
                    return Ok(());
                }
                state.mls.epoch()?
            };
            let Some(bytes) = self.inner.bucket.commit(epoch).await? else {
                return Ok(());
            };
            let commit = SignedCommit::verify(&bytes, epoch, &admin)
                .with_context(|| format!("ignoring invalid commit for epoch {epoch}"))?;
            let outcome = {
                let mut state = self.state();
                let outcome = state.mls.process_commit(&commit, &admin)?;
                state.record_epoch()?;
                outcome
            };
            self.emit(Change::Members);
            if outcome.removed_self {
                self.inner.status.lock().expect("status lock poisoned").removed = true;
                self.emit(Change::Removed);
                return Ok(());
            }
        }
    }

    async fn admin_duties(&self) -> Result<()> {
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
                        keyring: Some(seal_keyring(&mut state)?),
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
            let delivery: Delivery = postcard::from_bytes(&payload)?;
            if self.state().store.has_ops_from(&joiner)? {
                self.state().store.delete_delivery(&joiner)?;
                continue;
            }
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

    /// Signs a staged commit and saves it locally before it is applied, so a
    /// crash can't leave us in an epoch the bucket never hears about.
    fn record_commit(&self, state: &State, from_epoch: u64, commit: Vec<u8>) -> Result<Vec<u8>> {
        let signature = self.inner.identity.sign(&commit_message(from_epoch, &commit));
        let signed = postcard::to_stdvec(&SignedCommit {
            from_epoch,
            commit,
            signature: signature.to_bytes().to_vec(),
        })?;
        state.store.put_commit_record(from_epoch, &signed)?;
        Ok(signed)
    }

    async fn upload_commit(&self, from_epoch: u64, signed: Vec<u8>) -> Result<()> {
        self.inner.bucket.put_commit(from_epoch, signed).await?;
        if let Some(net) = &self.inner.net {
            let _ = net.nudge().await;
        }
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

    async fn pull(&self) -> Result<()> {
        let me = self.me();
        let authors: Vec<_> = self
            .members()?
            .iter()
            .filter_map(|member| parse_member_id(&member.id).ok())
            .filter(|author| *author != me)
            .collect();
        for author in authors {
            let cursor = self.state().store.cursor(&author)?;
            for (key, envelopes) in self
                .inner
                .bucket
                .segments_after(&author, cursor.as_deref())
                .await?
            {
                for envelope in envelopes {
                    if envelope.author != author {
                        warn!("segment {key} contains an op by someone else; skipping it");
                        continue;
                    }
                    if let Err(error) = self.receive(envelope) {
                        warn!("dropping op from {key}: {error:#}");
                    }
                }
                self.state().store.set_cursor(&author, &key)?;
            }
        }
        Ok(())
    }

    async fn push(&self) -> Result<()> {
        let me = self.me();
        let unpushed = self.state().store.unpushed(&me)?;
        let Some(last) = unpushed.last().map(|envelope| envelope.seq) else {
            return Ok(());
        };
        self.inner.bucket.put_segment(&unpushed).await?;
        self.state().store.mark_pushed(&me, last)?;
        Ok(())
    }

    /// Takes an envelope from gossip or the bucket. It is stored only once it
    /// verifies; envelopes we can't apply yet are parked and retried.
    pub(crate) fn receive(&self, envelope: Envelope) -> Result<()> {
        let change = {
            let state = self.state();
            ensure!(
                envelope.seq <= MAX_STORED_NUMBER && envelope.epoch <= MAX_STORED_NUMBER,
                "op numbering out of range"
            );
            if state.store.is_applied(&envelope.author, envelope.seq)? {
                return Ok(());
            }
            match verify(&state, &envelope)? {
                Some(verified) if state.store.accepts_clock(verified.body.lamport)? => {
                    let change = state.store.apply(&verified)?;
                    state.store.insert_applied(&envelope, true)?;
                    Some(change)
                }
                _ => {
                    ensure!(
                        state.store.was_ever_member(&envelope.author)?,
                        "op from a non-member"
                    );
                    let current_epoch = state.mls.epoch()?;
                    ensure!(
                        envelope.epoch <= current_epoch + MAX_PENDING_EPOCHS_AHEAD,
                        "op is for an epoch too far ahead ({})",
                        envelope.epoch
                    );
                    state.store.insert_pending(&envelope)?;
                    None
                }
            }
        };
        if let Some(change) = change {
            self.emit(change);
        }
        Ok(())
    }

    fn apply_pending(&self) -> Result<()> {
        let changes = {
            let state = self.state();
            let mut changes = Vec::new();
            for envelope in state.store.pending()? {
                if state.store.is_applied(&envelope.author, envelope.seq)? {
                    state.store.delete_pending(&envelope)?;
                    continue;
                }
                match verify(&state, &envelope) {
                    Ok(Some(verified)) if state.store.accepts_clock(verified.body.lamport)? => {
                        changes.push(state.store.apply(&verified)?);
                        state.store.insert_applied(&envelope, true)?;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        warn!("dropping pending op: {error:#}");
                        state.store.delete_pending(&envelope)?;
                    }
                }
            }
            changes
        };
        for change in changes {
            self.emit(change);
        }
        Ok(())
    }
}

/// `Ok(None)` when we lack the epoch key. Otherwise the op must decrypt,
/// carry a valid signature, and come from a member of that epoch.
fn verify(state: &State, envelope: &Envelope) -> Result<Option<VerifiedOp>> {
    let Some(key) = state.store.epoch_key(envelope.epoch)? else {
        return Ok(None);
    };
    ensure!(
        state.store.was_member_at(envelope.epoch, &envelope.author)?,
        "author was not a member in epoch {}",
        envelope.epoch
    );
    Ok(Some(crypto::open(envelope, &key)?))
}
