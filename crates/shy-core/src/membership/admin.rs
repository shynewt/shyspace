//! The admin's duties: inviting people, deciding on join requests, removing
//! members, and keeping the bucket's copy of every commit intact.

use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::{
    commit::{self, FrozenAuthor},
    device::DeviceCertificate,
    invite::{Answer, Grant, GrantWithDevices, Invite, JoinRequest, accepted_bytes, declined_bytes},
    keyring::KeyHistory,
    request_seal::{NotForThisInvite, RequestSlot, is_unsealed_request, open_request, request_key_pair},
};
use crate::{
    config,
    ops::{new_random_id, now_ms},
    storage::Store,
    transport::bucket::InviteLinks,
    views::{self, Change, InviteState, parse_member_id, safety_code, shown_safety_code},
    workspace::{State, Workspace},
};

/// Presigned S3 links can't outlive seven days.
const INVITE_LIFETIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_REQUESTER_NAME: usize = 80;
/// Refused join requests remembered so each is logged once; one per open
/// invite is the most anyone but an attacker leaves.
const MAX_REMEMBERED_REFUSALS: usize = 1024;
/// At most this many devices linked to one person within `LINK_WINDOW_MS`,
/// by this device's clock.
const MAX_LINKS_PER_DAY: usize = 5;
const LINK_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
/// How often a pending join answer is put up again, in case someone removed it.
const ANSWER_REFRESH: Duration = Duration::from_secs(10 * 60);

/// What the admin still owes a joiner. Saved before the commit is applied and
/// kept until the joiner posts, so neither a crash nor a failed upload strands
/// them; re-uploading also repairs an answer someone overwrote.
#[derive(Serialize, Deserialize)]
pub(crate) struct Delivery {
    pub welcome: Vec<u8>,
    /// Sealed right after the commit is applied; empty until then.
    pub sealed_grant: Option<Vec<u8>>,
    /// The device map's digest the add was committed with, once the
    /// workspace has a linked device.
    pub map_digest: Option<[u8; 32]>,
    /// The joiner reads a signed acceptance, so it gets one.
    pub is_signed: bool,
}

/// A delivery saved by a version from before linked devices.
#[derive(Deserialize)]
struct DeliveryBeforeDevices {
    welcome: Vec<u8>,
    sealed_grant: Option<Vec<u8>>,
}

impl Delivery {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if let Ok(delivery) = postcard::from_bytes::<Self>(bytes) {
            return Ok(delivery);
        }
        let older: DeliveryBeforeDevices = postcard::from_bytes(bytes)?;
        Ok(Self {
            welcome: older.welcome,
            sealed_grant: older.sealed_grant,
            map_digest: None,
            is_signed: false,
        })
    }
}

/// What a newly linked device needs to finish joining (see
/// [`DeviceEnrollment::finish`](crate::DeviceEnrollment::finish)). Who the
/// admin is comes from the person's device instead, in the
/// [`DeviceLink`](crate::DeviceLink).
#[derive(Debug, Clone)]
pub struct DeviceWelcome {
    pub welcome: Vec<u8>,
    pub sealed_grant: Vec<u8>,
    /// The device map's digest the commit was made with, which the Welcome
    /// needs and the grant must match.
    pub map_digest: [u8; 32],
}

/// The device was added, but its commit isn't in the bucket yet; the next
/// sync pass puts it there. The welcome is still the device's: hand it over
/// once a pass has published the commit (the device waits for it either way).
#[derive(Debug)]
pub struct UnpublishedDevice {
    pub welcome: DeviceWelcome,
    pub error: anyhow::Error,
}

impl std::fmt::Display for UnpublishedDevice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the device was added, but the change isn't uploaded yet; hand the device its welcome once a sync has \
             uploaded it: {:#}",
            self.error
        )
    }
}

impl std::error::Error for UnpublishedDevice {}

/// Same-named people are told apart by the start of their safety code, so
/// no new key may share it with a member's device or with a person, even
/// one whose first device is gone; trying keys can make one that does.
fn ensure_a_distinct_safety_code(state: &State, key: &iroh::PublicKey) -> Result<()> {
    let shown = shown_safety_code(&safety_code(key));
    let people = state
        .store
        .device_records()?
        .into_iter()
        .map(|record| record.certificate.person);
    for existing in state.mls.members()?.into_iter().chain(people) {
        ensure!(
            shown_safety_code(&safety_code(&existing)) != shown,
            "this request's safety code starts like a member's ({shown}), which could let them pass for that \
             member; decline it and ask them to join again: a new request gets a new code"
        );
    }
    Ok(())
}

#[cfg(test)]
pub(crate) type GrantTamper = Box<dyn FnOnce(&mut GrantWithDevices) + Send>;

/// What goes into the next commit besides its proposals.
struct CommitBinding {
    /// The commit's authenticated data: its lists' hash.
    lists: [u8; 32],
    /// The device map's digest, for a commit that binds the map.
    map_key: Option<[u8; 32]>,
}

/// After a crash before a join commit was recorded, its Welcome is useless.
pub(crate) fn drop_unsealed_deliveries(store: &Store) -> Result<()> {
    for (joiner, _, payload) in store.deliveries()? {
        if Delivery::decode(&payload)?.sealed_grant.is_none() {
            store.delete_delivery(&joiner)?;
        }
    }
    Ok(())
}

impl Workspace {
    pub(crate) fn ensure_admin(&self) -> Result<()> {
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
            request_key: request_key_pair(&self.inner.identity, &token)?.public,
            signs_acceptance: true,
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
        // No pass may upload the commit before the snapshot below covers it.
        let _no_pass = self.inner.sync_lock.lock().await;
        let (from_epoch, signed) = {
            let mut state = self.state();
            let pending = state
                .store
                .pending_request(token, now_ms()?)?
                .context("this invite has no request waiting for approval")?;
            let request = JoinRequest::decode(&pending.request)?;
            let (claimed, key_package) = state
                .mls
                .verify_key_package(&request.key_package, &request.binding)?;
            ensure!(
                claimed == pending.joiner,
                "the stored request doesn't match its requester"
            );
            ensure!(!state.mls.is_member(&claimed)?, "this person is already a member");
            ensure_a_distinct_safety_code(&state, &claimed)?;

            let from_epoch = state.mls.epoch()?;
            let binding = self.bind_next_commit(&state, from_epoch, &[], &[])?;
            ensure!(
                binding.map_key.is_none() || request.reads_signed_answers,
                "this request comes from an older version of shyspace, which can't join a workspace with linked \
                 devices; decline it and ask them to update, then join again"
            );
            let (commit, welcome) =
                state
                    .mls
                    .add_member(key_package, &binding.lists, binding.map_key.as_ref())?;
            let delivery = Delivery {
                welcome,
                sealed_grant: None,
                map_digest: binding.map_key,
                is_signed: request.reads_signed_answers,
            };
            state
                .store
                .put_delivery(&claimed, token, &postcard::to_stdvec(&delivery)?)?;
            let signed = self.record_commit(&state, from_epoch, commit, Vec::new(), &[], &binding)?;
            state.mls.merge_pending_commit()?;
            state.record_epoch(&self.inner.config)?;
            let grant = Grant {
                history: KeyHistory::collect(&state.store)?,
                bucket: config::current_bucket(&state.store)?,
                bucket_lamport: config::bucket_lamport(&state.store)?,
            };
            let grant = match delivery.map_digest {
                Some(_) => postcard::to_stdvec(&self.grant_with_devices(&state.store, grant)?)?,
                None => postcard::to_stdvec(&grant)?,
            };
            let sealed_grant = state.mls.seal_for_members(&grant)?;
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
        let requester = {
            let state = self.state();
            let requester = state.store.requester(token)?;
            let changed =
                state
                    .store
                    .change_invite_state(token, &[InviteState::Requested], InviteState::Rejected)?;
            ensure!(changed, "this invite has no request waiting for a decision");
            requester.context("this invite's request names no one")?
        };
        self.emit(Change::Invites);
        let bytes = declined_bytes(&self.inner.config.workspace_id, token, &requester)?;
        let signature = self.inner.identity.sign(&bytes).to_bytes().to_vec();
        self.bucket()
            .put_invite_answer(token, postcard::to_stdvec(&Answer::Rejected { signature })?)
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

    /// Links a new device to the person whose device vouched for it in
    /// `certificate`: adds it to the group in a commit that carries the
    /// certificate, once the certificate meets every rule members check.
    /// The returned welcome is all the device needs to finish joining.
    pub async fn add_device(
        &self,
        key_package: &[u8],
        binding: &[u8],
        certificate: &DeviceCertificate,
    ) -> Result<DeviceWelcome> {
        self.ensure_admin()?;
        // No pass may upload the commit before the snapshot below covers it.
        let _no_pass = self.inner.sync_lock.lock().await;
        let workspace_id = &self.inner.config.workspace_id;
        let certificates = std::slice::from_ref(certificate);
        let (from_epoch, signed, welcome) = {
            let mut state = self.state();
            let (device, key_package) = state.mls.verify_key_package(key_package, binding)?;
            ensure!(
                device == certificate.device,
                "the device's request doesn't match its certificate"
            );
            ensure!(!state.mls.is_member(&device)?, "this device is already a member");
            ensure_a_distinct_safety_code(&state, &device)?;
            // Each link costs every member an epoch and a longer history.
            let day_ago = now_ms()? - LINK_WINDOW_MS;
            ensure!(
                state.store.links_since(&certificate.person, day_ago)? < MAX_LINKS_PER_DAY,
                "this person has linked {MAX_LINKS_PER_DAY} devices in the last day; try again tomorrow"
            );
            let from_epoch = state.mls.epoch()?;
            let mut members_after = state.mls.members()?;
            members_after.push(device);
            let counted =
                state
                    .store
                    .count_certificates(workspace_id, from_epoch, &members_after, certificates)?;
            ensure!(
                counted.len() == 1,
                "this device can't be linked: its certificate doesn't meet the rules every member checks"
            );

            let binding = self.bind_next_commit(&state, from_epoch, &[], certificates)?;
            let map_digest = binding
                .map_key
                .context("a commit that adds a device binds the map")?;
            let (commit, welcome) = state
                .mls
                .add_member(key_package, &binding.lists, Some(&map_digest))?;
            // The digest covers the map before the commit.
            let devices = state.store.device_records()?;
            let bound_frozen = state.store.bound_freezes()?;
            let signed =
                self.record_commit(&state, from_epoch, commit, Vec::new(), certificates, &binding)?;
            state.mls.merge_pending_commit()?;
            state.record_epoch(&self.inner.config)?;
            let grant = GrantWithDevices {
                grant: Grant {
                    history: KeyHistory::collect(&state.store)?,
                    bucket: config::current_bucket(&state.store)?,
                    bucket_lamport: config::bucket_lamport(&state.store)?,
                },
                devices,
                bound_frozen,
            };
            #[cfg(test)]
            let grant = self.tampered(grant);
            let sealed_grant = state.mls.seal_for_members(&postcard::to_stdvec(&grant)?)?;
            Workspace::mark_recovery_snapshot_stale(&state.store)?;
            let welcome = DeviceWelcome {
                welcome,
                sealed_grant,
                map_digest,
            };
            (from_epoch, signed, welcome)
        };
        self.emit(Change::Members);
        // Snapshot first: a commit must never be in the bucket before the
        // recovery snapshot that includes it.
        let published = async {
            self.refresh_recovery_snapshot().await?;
            self.upload_commit(from_epoch, signed).await
        }
        .await;
        match published {
            Ok(()) => Ok(welcome),
            Err(error) => Err(UnpublishedDevice { welcome, error }.into()),
        }
    }

    /// Picks up join requests and delivers answers owed.
    pub(crate) async fn answer_join_requests(&self) -> Result<()> {
        self.collect_join_requests().await?;
        self.deliver_answers().await
    }

    /// Picks up requests posted through open invites, for the admin to decide.
    /// Reads the request slot of every open invite (none once it is used,
    /// revoked or expired); returns whether a request was recorded.
    pub(crate) async fn collect_join_requests(&self) -> Result<bool> {
        let tokens = self.state().store.open_invite_tokens(now_ms()?)?;
        let mut has_recorded = false;
        for token in tokens {
            let Some(bytes) = self.bucket().invite_request(&token).await? else {
                continue;
            };
            // One refused stays: a joiner's upload replaces junk, and
            // deleting could delete a genuine request posted meanwhile.
            match self.record_request(&token, &bytes) {
                Ok(()) => {
                    has_recorded = true;
                    self.emit(Change::Invites);
                }
                Err(_) if !self.is_new_refused_request(&token, &bytes) => {}
                Err(_) if is_unsealed_request(&bytes) => warn!(
                    "a join request came unsealed, from a version of shyspace older than 0.1.4; send that person a new invite"
                ),
                Err(error) if error.downcast_ref::<NotForThisInvite>().is_some() => {
                    debug!("ignoring a join request: {error:#}");
                }
                Err(error) => warn!("couldn't take a join request: {error:#}"),
            }
        }
        Ok(has_recorded)
    }

    /// Whether this refused request is new in its slot, so each is logged
    /// once rather than at every check while it sits there (each check
    /// still tries it again).
    fn is_new_refused_request(&self, token: &str, bytes: &[u8]) -> bool {
        let mut refused = self
            .inner
            .refused_requests
            .lock()
            .expect("refused requests lock poisoned by an earlier panic");
        if refused.len() >= MAX_REMEMBERED_REFUSALS {
            refused.clear();
        }
        let seen = blake3::Hasher::new()
            .update(&(token.len() as u64).to_le_bytes())
            .update(token.as_bytes())
            .update(bytes)
            .finalize();
        refused.insert(*seen.as_bytes())
    }

    fn record_request(&self, token: &str, bytes: &[u8]) -> Result<()> {
        let config = &self.inner.config;
        let slot = RequestSlot {
            workspace_id: &config.workspace_id,
            token,
            admin: &config.admin,
        };
        let request = open_request(&self.inner.identity, slot, bytes)?;
        let opened = postcard::to_stdvec(&request)?;
        let state = self.state();
        let (joiner, _) = state
            .mls
            .verify_key_package(&request.key_package, &request.binding)?;
        let name: String = request.name.trim().chars().take(MAX_REQUESTER_NAME).collect();
        ensure!(!name.is_empty(), "the request has no name");
        state
            .store
            .record_join_request(token, &joiner, &name, request.is_bot, &opened)
    }

    /// Uploads owed answers until each joiner posts their first op (or is
    /// removed first), then clears the invite's slots in the bucket. An
    /// answer goes up once, and again only every few minutes, in case
    /// someone with the bucket keys removed it. One failure doesn't hold
    /// back the other answers; the first is returned at the end.
    async fn deliver_answers(&self) -> Result<()> {
        let deliveries = self.state().store.deliveries()?;
        let mut first_error = None;
        for (joiner, token, payload) in deliveries {
            if let Err(error) = self.deliver_answer(joiner, &token, &payload).await {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn deliver_answer(&self, joiner: iroh::PublicKey, token: &str, payload: &[u8]) -> Result<()> {
        if self.state().store.has_ops_from(&joiner)? {
            return self.end_delivery(joiner, token).await;
        }
        let delivery = Delivery::decode(payload)?;
        let Some(sealed_grant) = delivery.sealed_grant else {
            warn!("no grant sealed for {joiner}; they need a new invite");
            self.state().store.delete_delivery(&joiner)?;
            self.answers_sent().remove(&joiner);
            return Ok(());
        };
        // A sealed grant means they were added, so not being a member now
        // means they were removed before they ever posted.
        if !self.state().mls.is_member(&joiner)? {
            return self.end_delivery(joiner, token).await;
        }
        let is_recent = self
            .answers_sent()
            .get(&joiner)
            .is_some_and(|sent| sent.elapsed() < ANSWER_REFRESH);
        if is_recent {
            return Ok(());
        }
        let answer = match delivery.is_signed {
            true => {
                let bytes = accepted_bytes(
                    &self.inner.config.workspace_id,
                    token,
                    &joiner,
                    delivery.map_digest.as_ref(),
                    &delivery.welcome,
                    &sealed_grant,
                )?;
                Answer::AcceptedSigned {
                    signature: self.inner.identity.sign(&bytes).to_bytes().to_vec(),
                    welcome: delivery.welcome,
                    sealed_grant,
                    map_digest: delivery.map_digest,
                }
            }
            false => Answer::Accepted {
                welcome: delivery.welcome,
                sealed_grant,
            },
        };
        self.upload_answer(token, &answer).await?;
        self.answers_sent().insert(joiner, tokio::time::Instant::now());
        Ok(())
    }

    async fn end_delivery(&self, joiner: iroh::PublicKey, token: &str) -> Result<()> {
        self.bucket().delete_invite_answer(token).await?;
        self.bucket().delete_invite_request(token).await?;
        self.state().store.delete_delivery(&joiner)?;
        self.answers_sent().remove(&joiner);
        Ok(())
    }

    async fn upload_answer(&self, token: &str, answer: &Answer) -> Result<()> {
        #[cfg(test)]
        if self
            .inner
            .fail_next_answer_upload
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            bail!("a simulated failed upload");
        }
        self.bucket()
            .put_invite_answer(token, postcard::to_stdvec(answer)?)
            .await
    }

    fn answers_sent(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<iroh::PublicKey, tokio::time::Instant>> {
        self.inner
            .answers_sent
            .lock()
            .expect("sent answers lock poisoned by an earlier panic")
    }

    /// Removes a person with every device of theirs, in one commit. Each
    /// device's log is frozen at the last op we have from it, so we first
    /// pull everything they wrote.
    pub async fn remove_member(&self, member_id: &str) -> Result<()> {
        self.ensure_admin()?;
        let devices = {
            let state = self.state();
            let person = state.store.person_of(&parse_member_id(member_id)?)?;
            if person == state.store.person_of(&self.me())? {
                bail!(
                    "the admin cannot remove themselves; to remove one of their other devices, remove that device"
                );
            }
            let mut devices = Vec::new();
            for member in state.mls.members()? {
                if state.store.person_of(&member)? == person {
                    devices.push(member);
                }
            }
            ensure!(!devices.is_empty(), "not a member");
            devices
        };
        self.remove_leaves(devices).await
    }

    /// Removes these devices in one commit, as when a stolen device and the
    /// devices it linked go together. Their persons keep any other devices.
    pub async fn remove_devices(&self, device_ids: &[String]) -> Result<()> {
        self.ensure_admin()?;
        let mut devices = device_ids
            .iter()
            .map(|id| parse_member_id(id))
            .collect::<Result<Vec<_>>>()?;
        devices.sort_unstable();
        devices.dedup();
        ensure!(!devices.is_empty(), "no devices to remove");
        ensure!(
            !devices.contains(&self.me()),
            "the admin cannot remove the device they're using"
        );
        {
            let state = self.state();
            for device in &devices {
                ensure!(state.mls.is_member(device)?, "not a member");
            }
        }
        self.remove_leaves(devices).await
    }

    async fn remove_leaves(&self, devices: Vec<iroh::PublicKey>) -> Result<()> {
        // No pass may upload the commit before the snapshot below covers it.
        let _no_pass = self.inner.sync_lock.lock().await;
        for device in &devices {
            self.collect_removed_log(device)
                .await
                .context("can't collect the member's last messages")?;
        }
        // One hold from reading the last seqs to settling the freeze, so no
        // op can land in between.
        let (from_epoch, signed) = {
            let mut state = self.state();
            let frozen = devices
                .iter()
                .map(|member| {
                    Ok(FrozenAuthor {
                        member: *member,
                        last_seq: state.store.last_seq(member)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let from_epoch = state.mls.epoch()?;
            let binding = self.bind_next_commit(&state, from_epoch, &frozen, &[])?;
            let commit = state
                .mls
                .remove_members(&devices, &binding.lists, binding.map_key.as_ref())?;
            let signed = self.record_commit(&state, from_epoch, commit, frozen, &[], &binding)?;
            state.mls.merge_pending_commit()?;
            // Freezing comes after the merge, as on every member's device.
            state.record_epoch(&self.inner.config)?;
            Workspace::mark_recovery_snapshot_stale(&state.store)?;
            Workspace::start_storage_rotation(&state.store)?;
            (from_epoch, signed)
        };
        self.emit(Change::Members);
        // The removal is made; views catch up here or at the next pass.
        if let Err(error) = self.finish_view_work() {
            warn!("the views will catch up with the removal at the next pass: {error:#}");
        }
        self.refresh_recovery_snapshot().await?;
        self.upload_commit(from_epoch, signed).await
    }

    /// The grant with the device map and bound freezes, as they stand once
    /// the add is settled.
    fn grant_with_devices(&self, store: &Store, grant: Grant) -> Result<GrantWithDevices> {
        let with_devices = GrantWithDevices {
            grant,
            devices: store.device_records()?,
            bound_frozen: store.bound_freezes()?,
        };
        #[cfg(test)]
        let with_devices = self.tampered(with_devices);
        Ok(with_devices)
    }

    #[cfg(test)]
    fn tampered(&self, mut with_devices: GrantWithDevices) -> GrantWithDevices {
        let tamper = self
            .inner
            .tamper_next_grant
            .lock()
            .expect("grant tamper lock poisoned")
            .take();
        if let Some(tamper) = tamper {
            tamper(&mut with_devices);
        }
        with_devices
    }

    /// How the next commit binds its lists, and, once the workspace has a
    /// device or the commit adds one, the device map.
    fn bind_next_commit(
        &self,
        state: &State,
        from_epoch: u64,
        frozen: &[FrozenAuthor],
        certificates: &[DeviceCertificate],
    ) -> Result<CommitBinding> {
        let from_epoch_key = state.mls.epoch_key()?;
        let binds_map = !certificates.is_empty() || state.store.has_devices()?;
        Ok(CommitBinding {
            lists: commit::lists_hash(&from_epoch_key, from_epoch, frozen, certificates)?,
            map_key: binds_map
                .then(|| state.map_key(&self.inner.config.workspace_id, from_epoch, frozen))
                .transpose()?,
        })
    }

    /// Signs a staged commit and saves it before it is applied, so a crash
    /// can't leave us in an epoch the bucket never hears about.
    fn record_commit(
        &self,
        state: &State,
        from_epoch: u64,
        commit: Vec<u8>,
        frozen: Vec<FrozenAuthor>,
        certificates: &[DeviceCertificate],
        binding: &CommitBinding,
    ) -> Result<Vec<u8>> {
        let signed = match binding.map_key {
            Some(_) => commit::sign_v3(
                &self.inner.identity,
                &state.mls.epoch_key()?,
                from_epoch,
                commit,
                frozen,
                certificates,
            )?,
            None => commit::sign(&self.inner.identity, from_epoch, commit, frozen)?,
        };
        state.save_commit(from_epoch, &signed)?;
        Ok(signed)
    }

    async fn upload_commit(&self, from_epoch: u64, signed: Vec<u8>) -> Result<()> {
        #[cfg(test)]
        if self
            .inner
            .fail_next_commit_upload
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            anyhow::bail!("the bucket can't be reached");
        }
        self.bucket().put_commit(from_epoch, signed).await?;
        self.nudge_peers().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delivery_saved_before_linked_devices_still_reads() {
        #[derive(Serialize)]
        struct Saved {
            welcome: Vec<u8>,
            sealed_grant: Option<Vec<u8>>,
        }
        let saved = postcard::to_stdvec(&Saved {
            welcome: b"welcome".to_vec(),
            sealed_grant: Some(b"grant".to_vec()),
        })
        .unwrap();

        let delivery = Delivery::decode(&saved).unwrap();

        assert_eq!(delivery.welcome, b"welcome");
        assert_eq!(delivery.sealed_grant.unwrap(), b"grant");
        assert_eq!(delivery.map_digest, None);
        assert!(!delivery.is_signed);
    }
}
