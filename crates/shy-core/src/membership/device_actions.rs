//! The admin's app acting on device changes, after each sync pass (each
//! commit takes the sync lock itself). It first finishes what earlier passes
//! owe, then acts on the changes it hasn't acted on: removals first, in one
//! commit, then links. Each action is marked acted, with the work it owes,
//! in one transaction, so neither a crash nor a restore makes it act twice
//! or leaves a welcome or a key-only commit behind.

use anyhow::{Context, Result};
use iroh::PublicKey;
use tracing::warn;

use super::{
    admin::{
        ANSWER_REFRESH, AddedDevice, DeviceWelcome, LINK_WINDOW_MS, MAX_LINKS_PER_DAY, UnpublishedDevice,
    },
    device::DeviceCertificate,
    device_change::DeviceChange,
    link::{LinkedDevice, Token},
    link_answers::{Outcome, Reason, SignedOutcome, SignedWelcome},
};
use crate::{
    ops::now_ms,
    storage::{
        Acted, CERTIFICATE_LIFETIME_EPOCHS, MAX_DEVICES_PER_PERSON, OwedWelcome, RecordedChange, Store,
    },
    workspace::Workspace,
};

/// How long the admin's app keeps handing out a link's welcome.
const WELCOME_LIFETIME_MS: i64 = 7 * 24 * 60 * 60 * 1000;

impl Workspace {
    pub(crate) async fn act_on_device_changes(&self) -> Result<()> {
        if !self.is_admin() {
            return Ok(());
        }
        let _one_at_a_time = self.inner.device_actions_lock.lock().await;
        if self.state().store.is_catching_up()? {
            return Ok(());
        }
        self.finish_owed_device_work().await?;
        let pending = self.state().store.unacted_device_changes()?;
        if pending.is_empty() {
            return Ok(());
        }
        self.act_on_removals(&pending).await?;
        self.act_on_links(&pending).await?;
        // A refusal owes a key-only commit, made here even if the pass
        // committed since.
        self.finish_owed_device_work().await
    }

    /// Welcomes first: a grant not sealed yet can only be sealed before the
    /// next commit moves the epoch.
    async fn finish_owed_device_work(&self) -> Result<()> {
        self.deliver_link_welcomes().await?;
        self.upload_link_outcomes().await?;
        if self.state().store.is_key_only_commit_owed()? {
            self.commit_keys_only().await?;
        }
        Ok(())
    }

    /// Whether the change's author can still have it acted on: a current
    /// member, not frozen by a removal acted on first.
    fn may_still_act_for(&self, change: &RecordedChange) -> Result<bool> {
        let state = self.state();
        Ok(state.mls.is_member(&change.author)? && state.store.frozen_last_seq(&change.author)?.is_none())
    }

    fn member_devices_of(&self, person: &PublicKey) -> Result<Vec<PublicKey>> {
        let state = self.state();
        let mut members = Vec::new();
        for device in state.store.devices_of(person)? {
            if state.mls.is_member(&device)? {
                members.push(device);
            }
        }
        Ok(members)
    }

    /// Removes, in one commit, what the removals ask for, taking the first
    /// devices' and the admin's requests first and skipping a request whose
    /// author the batch already removes. A named key that isn't a member
    /// changes nothing here: it cancels a link, when that link is acted on.
    async fn act_on_removals(&self, pending: &[RecordedChange]) -> Result<()> {
        let admin = self.inner.config.admin;
        let mut removals: Vec<(&RecordedChange, &Vec<PublicKey>)> = pending
            .iter()
            .filter_map(|change| match &change.change {
                DeviceChange::Remove { devices } => Some((change, devices)),
                DeviceChange::Link { .. } => None,
            })
            .collect();
        // Stable: within each group, the order they were applied in.
        removals.sort_by_key(|(change, _)| change.author != change.person && change.author != admin);
        let mut batch: Vec<PublicKey> = Vec::new();
        let mut removed_by: Vec<&RecordedChange> = Vec::new();
        let mut skipped: Vec<&RecordedChange> = Vec::new();
        for (change, devices) in removals {
            if batch.contains(&change.author) || !self.may_still_act_for(change)? {
                skipped.push(change);
                continue;
            }
            let mut targets = Vec::new();
            for device in devices {
                if batch.contains(device) {
                    continue;
                }
                // Judged as of the op's epoch, a key may have been no one's
                // then and someone else's now: only the requester's own
                // devices go.
                let (is_member, person) = {
                    let state = self.state();
                    (state.mls.is_member(device)?, state.store.person_of(device)?)
                };
                if is_member && person == change.person {
                    targets.push(*device);
                }
            }
            // Removing a person's last device would be leaving the workspace.
            let is_last = self
                .member_devices_of(&change.person)?
                .iter()
                .all(|device| batch.contains(device) || targets.contains(device));
            if is_last && !targets.is_empty() {
                skipped.push(change);
                continue;
            }
            batch.extend(targets);
            removed_by.push(change);
        }
        if removed_by.is_empty() && skipped.is_empty() {
            return Ok(());
        }
        let now = now_ms()?;
        let mark_all = |store: &Store| -> Result<()> {
            for change in &removed_by {
                store.mark_acted(change, Acted::Removed, now)?;
            }
            for change in &skipped {
                store.mark_acted(change, Acted::Skipped, now)?;
            }
            Workspace::mark_recovery_snapshot_stale(store)
        };
        match batch.is_empty() {
            true => self.state().store.atomically(mark_all)?,
            false => self.remove_leaves_with(batch, mark_all).await?,
        }
        Ok(())
    }

    /// Adds, one commit each, the devices the links ask for.
    async fn act_on_links(&self, pending: &[RecordedChange]) -> Result<()> {
        for change in pending {
            let DeviceChange::Link {
                token,
                linked,
                certificate,
            } = &change.change
            else {
                continue;
            };
            let is_done = self.state().store.was_ever_member(&certificate.device)?;
            if is_done || !self.may_still_act_for(change)? {
                self.state()
                    .store
                    .atomically(|store| store.mark_acted(change, Acted::Skipped, now_ms()?))?;
                continue;
            }
            let reason = match self.is_cancelled(&change.person, &certificate.device)? {
                true => Some(Reason::Cancelled),
                false => self.reason_to_refuse(certificate)?,
            };
            match reason {
                Some(reason) => self.refuse_link(change, token, certificate, reason)?,
                None => self.add_linked_device(change, token, linked, certificate).await?,
            }
        }
        Ok(())
    }

    /// Whether a removal `person` asked for, which the admin's app acted on,
    /// names `device`: in this pass (removals go first) or an earlier one. A
    /// device key is new for each link and never comes back, so one named
    /// before it was added can only mean "cancel". A skipped removal (its
    /// author removed or frozen) cancels nothing.
    fn is_cancelled(&self, person: &PublicKey, device: &PublicKey) -> Result<bool> {
        let store = &self.state().store;
        for change in store.device_changes()? {
            let names_it =
                matches!(&change.change, DeviceChange::Remove { devices } if devices.contains(device));
            if change.person == *person && names_it && store.acted_on(&change)? == Some(Acted::Removed) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// What the admin's app can tell before trying, so each refusal says why.
    fn reason_to_refuse(&self, certificate: &DeviceCertificate) -> Result<Option<Reason>> {
        let epoch = self.state().mls.epoch()?;
        if epoch.saturating_sub(certificate.epoch) > CERTIFICATE_LIFETIME_EPOCHS {
            return Ok(Some(Reason::Stale));
        }
        if self.member_devices_of(&certificate.person)?.len() >= MAX_DEVICES_PER_PERSON {
            return Ok(Some(Reason::TooManyDevices));
        }
        if self.is_past_daily_limit(&certificate.person)? {
            return Ok(Some(Reason::DailyLimit));
        }
        Ok(None)
    }

    /// Whether `person` used up the day's tries. Refusals count too, so a
    /// person's device can't churn epochs with links bound to be refused.
    fn is_past_daily_limit(&self, person: &PublicKey) -> Result<bool> {
        let day_ago = now_ms()? - LINK_WINDOW_MS;
        let state = self.state();
        let tries =
            state.store.links_since(person, day_ago)? + state.store.refusals_since(person, day_ago)?;
        Ok(tries >= MAX_LINKS_PER_DAY)
    }

    /// Adds the device, with its acted mark and the welcome it is owed in
    /// the commit's transaction, or refuses it if the add fails.
    async fn add_linked_device(
        &self,
        change: &RecordedChange,
        token: &Token,
        linked: &LinkedDevice,
        certificate: &DeviceCertificate,
    ) -> Result<()> {
        let now = now_ms()?;
        let device = certificate.device;
        let owe_welcome = |store: &Store, added: &AddedDevice| -> Result<()> {
            store.mark_acted(change, Acted::Added, now)?;
            store.put_owed_welcome(&OwedWelcome {
                device,
                token: *token,
                welcome: added.welcome.clone(),
                map_digest: added.map_digest,
                epoch: added.epoch,
                devices_before: added.devices_before.clone(),
                bound_frozen_before: added.bound_frozen_before.clone(),
                sealed_grant: None,
                until_ms: now + WELCOME_LIFETIME_MS,
            })?;
            Workspace::mark_recovery_snapshot_stale(store)
        };
        let added = self
            .add_device_with(&linked.key_package, &linked.binding, certificate, owe_welcome)
            .await;
        let welcome = match added {
            Ok(welcome) => welcome,
            Err(error) => match error.downcast::<UnpublishedDevice>() {
                // Added; the next pass uploads its commit.
                Ok(unpublished) => unpublished.welcome,
                Err(error) => {
                    if self.state().mls.is_member(&device)? {
                        // Added before the error: the welcome's grant is
                        // made on a later pass.
                        warn!("a linked device was added, but its welcome isn't ready yet: {error:#}");
                        return Ok(());
                    }
                    warn!("refusing a link the admin's app couldn't make: {error:#}");
                    return self.refuse_link(change, token, certificate, Reason::Refused);
                }
            },
        };
        self.state().store.set_owed_grant(&device, &welcome.sealed_grant)
    }

    /// Marks the link acted on with the outcome it owes, and the key-only
    /// commit it owes, all in one transaction. Past the person's daily
    /// limit no refusal owes one, whatever its reason: as for
    /// [`Reason::DailyLimit`], the link's author already holds every key.
    fn refuse_link(
        &self,
        change: &RecordedChange,
        token: &Token,
        certificate: &DeviceCertificate,
        reason: Reason,
    ) -> Result<()> {
        let outcome = Outcome {
            token: *token,
            device: certificate.device,
            certificate_hash: certificate.hash()?,
            reason,
        };
        let signed =
            SignedOutcome::sign(&self.inner.identity, &self.inner.config.workspace_id, outcome)?.encode()?;
        let now = now_ms()?;
        let owes_key_only_commit =
            reason.needs_key_only_commit() && !self.is_past_daily_limit(&certificate.person)?;
        self.state().store.atomically(|store| {
            let acted = match reason.is_retry() {
                true => Acted::Stale,
                false => Acted::Refused,
            };
            store.mark_acted(change, acted, now)?;
            store.put_owed_outcome(token, &certificate.device, &signed)?;
            if owes_key_only_commit {
                store.owe_key_only_commit()?;
            }
            Workspace::mark_recovery_snapshot_stale(store)
        })
    }

    /// Puts each owed welcome up, again every few minutes in case someone
    /// with the bucket keys removed it, until it expires or its device is
    /// removed. A welcome whose grant isn't made yet gets it now, if the
    /// workspace is still in the epoch the add moved to.
    async fn deliver_link_welcomes(&self) -> Result<()> {
        let owed = self.state().store.owed_welcomes()?;
        for welcome in owed {
            let is_over = now_ms()? >= welcome.until_ms || !self.state().mls.is_member(&welcome.device)?;
            if is_over {
                // The slot first, so a failure leaves the row to try again.
                self.bucket()
                    .delete_link_reply(&welcome.token, &welcome.device, "welcome")
                    .await?;
                self.state().store.delete_owed_welcome(&welcome.device)?;
                self.welcomes_sent().remove(&(welcome.token, welcome.device));
                continue;
            }
            let Some(sealed_grant) = self.owed_grant(&welcome)? else {
                continue;
            };
            let is_recent = self
                .welcomes_sent()
                .get(&(welcome.token, welcome.device))
                .is_some_and(|sent| sent.elapsed() < ANSWER_REFRESH);
            if is_recent {
                continue;
            }
            let device_welcome = DeviceWelcome {
                welcome: welcome.welcome.clone(),
                sealed_grant,
                map_digest: welcome.map_digest,
            };
            let signed = SignedWelcome::sign(
                &self.inner.identity,
                &self.inner.config.workspace_id,
                &welcome.token,
                &welcome.device,
                &device_welcome,
            )?;
            self.bucket()
                .put_link_reply(&welcome.token, &welcome.device, "welcome", signed.encode()?)
                .await?;
            self.welcomes_sent()
                .insert((welcome.token, welcome.device), tokio::time::Instant::now());
        }
        Ok(())
    }

    /// The welcome's grant, made now if it wasn't: only possible in the
    /// epoch the add moved to. Past it, the device can't join from this
    /// welcome; its row goes, and it shows as never used.
    fn owed_grant(&self, welcome: &OwedWelcome) -> Result<Option<Vec<u8>>> {
        if let Some(sealed_grant) = &welcome.sealed_grant {
            return Ok(Some(sealed_grant.clone()));
        }
        let added = AddedDevice {
            welcome: welcome.welcome.clone(),
            map_digest: welcome.map_digest,
            epoch: welcome.epoch,
            devices_before: welcome.devices_before.clone(),
            bound_frozen_before: welcome.bound_frozen_before.clone(),
        };
        let mut state = self.state();
        match self.seal_device_grant(&mut state, &added) {
            Ok(sealed_grant) => {
                state.store.set_owed_grant(&welcome.device, &sealed_grant)?;
                Ok(Some(sealed_grant))
            }
            Err(error) => {
                warn!("a linked device's welcome can't be finished: {error:#}");
                state.store.delete_owed_welcome(&welcome.device)?;
                Ok(None)
            }
        }
    }

    fn welcomes_sent(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<(Token, PublicKey), tokio::time::Instant>> {
        self.inner
            .welcomes_sent
            .lock()
            .expect("welcomes sent lock poisoned by an earlier panic")
    }

    async fn upload_link_outcomes(&self) -> Result<()> {
        let owed = self.state().store.owed_outcomes()?;
        for (token, device, signed) in owed {
            self.bucket()
                .put_link_reply(&token, &device, "outcome", signed)
                .await
                .context("putting up a link's outcome")?;
            self.state().store.delete_owed_outcome(&token, &device)?;
        }
        Ok(())
    }
}
