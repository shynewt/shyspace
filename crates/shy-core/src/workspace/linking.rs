//! Linking a new device, as the device the person already uses sees it: it
//! makes a link code, opens the new device's request, shows the check code,
//! and, once the person confirms, vouches, asks the admin's app to add the
//! device, and hands the new device what it needs to read at once.
//!
//! One code is open at a time, and its secrets live only in memory, so a
//! restart ends it. Once confirmed, a link is saved, and what is left of it
//! (the request op, the hand-over) resumes after a restart.

use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};

use super::Workspace;
use crate::{
    config,
    membership::{
        device::DeviceCertificate,
        device_change::DeviceChange,
        handover::{Answer, Handover, seal_handover},
        keyring::KeyHistory,
        link::{
            ANSWER_LIFETIME_MS, LINK_CODE_LIFETIME_MS, LinkCode, LinkRequest, LinkSecrets, Platform,
            check_choices, make_check, open_link_request,
        },
    },
    ops::{Op, now_ms},
    storage::{MAX_DEVICES_PER_PERSON, PendingLink},
};

/// How long a confirmed link waits for the admin's app.
const PENDING_LINK_LIFETIME_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// The code this device shows, with what it keeps in memory for it.
pub(crate) struct OpenLink {
    code: LinkCode,
    /// Taken by the check: once the nonce is revealed, the code is spent.
    secrets: Option<LinkSecrets>,
    opened: Option<OpenedCheck>,
}

/// The request a code opened, with what the person picks from.
struct OpenedCheck {
    request: LinkRequest,
    /// What the new device shows.
    check_code: String,
    /// Made once, so they stay put while the person looks.
    choices: [String; 3],
}

/// What the person picks from before confirming: the code the new device
/// shows, among two decoys.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkCheckShown {
    pub choices: [String; 3],
    pub platform: Platform,
}

impl Workspace {
    /// Makes the code to show for linking a new device, replacing any open
    /// one. Refuses, with the reason, when linking can't work right now.
    pub async fn create_link_code(&self) -> Result<String> {
        self.ensure_may_link()?;
        let secrets = LinkSecrets::new()?;
        let links = self
            .bucket()
            .presign_link(
                &secrets.token(),
                Duration::from_millis(LINK_CODE_LIFETIME_MS as u64),
                Duration::from_millis(ANSWER_LIFETIME_MS as u64),
            )
            .await?;
        let code = self.link_code(&secrets, links.request_url, links.answer_url)?;
        let encoded = code.encode()?;
        self.open_link(code, secrets);
        Ok(encoded)
    }

    /// A link code without presigned links, since in-memory buckets can't sign.
    #[cfg(test)]
    pub(crate) fn create_test_link_code(&self) -> Result<LinkCode> {
        self.ensure_may_link()?;
        let secrets = LinkSecrets::new()?;
        let code = self.link_code(&secrets, String::new(), String::new())?;
        self.open_link(code.clone(), secrets);
        Ok(code)
    }

    fn open_link(&self, code: LinkCode, secrets: LinkSecrets) {
        *self.open_link_slot() = Some(OpenLink {
            code,
            secrets: Some(secrets),
            opened: None,
        });
    }

    fn open_link_slot(&self) -> std::sync::MutexGuard<'_, Option<OpenLink>> {
        self.inner
            .open_link
            .lock()
            .expect("open link lock poisoned by an earlier panic")
    }

    /// Checks what can be known before any code exists, so a link that
    /// would be refused never hands anything over.
    fn ensure_may_link(&self) -> Result<()> {
        let person = self.my_person();
        let state = self.state();
        ensure!(state.mls.is_active(), "this device isn't in the workspace");
        let pending = state.store.pending_links()?.len();
        let mut devices = 0;
        for device in state.store.devices_of(&person)? {
            if state.mls.is_member(&device)? {
                devices += 1;
            }
        }
        ensure!(
            devices + pending < MAX_DEVICES_PER_PERSON,
            "you already have {MAX_DEVICES_PER_PERSON} devices; remove one to link another"
        );
        let day_ago = now_ms()? - crate::membership::admin::LINK_WINDOW_MS;
        ensure!(
            state.store.links_since(&person, day_ago)? + pending
                < crate::membership::admin::MAX_LINKS_PER_DAY,
            "you've linked several devices today; try again tomorrow"
        );
        Ok(())
    }

    fn link_code(&self, secrets: &LinkSecrets, request_url: String, answer_url: String) -> Result<LinkCode> {
        let config = &self.inner.config;
        let admin_mls_key = self.state().mls.signature_key_of(&config.admin)?;
        Ok(LinkCode {
            workspace_id: config.workspace_id.clone(),
            workspace_name: config.name.clone(),
            admin: config.admin,
            admin_mls_key,
            person: self.my_person(),
            voucher: self.me(),
            token: secrets.token(),
            expires_ms: now_ms()? + LINK_CODE_LIFETIME_MS,
            request_url,
            answer_url,
            request_key: secrets.request_public_key().to_vec(),
        })
    }

    /// The check code to compare, once a new device has asked through the
    /// open code. The first request that opens gets the check, once; junk
    /// in the slot is ignored.
    pub async fn pending_link(&self) -> Result<Option<LinkCheckShown>> {
        let token = {
            let open = self.open_link_slot();
            let Some(open) = open.as_ref() else {
                return Ok(None);
            };
            if let Some(opened) = &open.opened {
                return Ok(Some(LinkCheckShown {
                    choices: opened.choices.clone(),
                    platform: opened.request.platform,
                }));
            }
            ensure!(!open.code.is_expired(now_ms()?), "this code has run out");
            open.code.token
        };
        let Some(bytes) = self.bucket().link_slot(&token, "request").await? else {
            return Ok(None);
        };
        let answer = {
            let mut open = self.open_link_slot();
            let Some(open) = open
                .as_mut()
                .filter(|open| open.code.token == token && open.opened.is_none())
            else {
                return Ok(None);
            };
            let Some(secrets) = open.secrets.take() else {
                return Ok(None);
            };
            let request = match open_link_request(&open.code, &secrets, &bytes) {
                Ok(request) => request,
                // Junk in the slot: the code stays open for the real request.
                Err(_) => {
                    open.secrets = Some(secrets);
                    return Ok(None);
                }
            };
            let (check, check_code) = make_check(&self.inner.identity, &open.code, secrets, &request)?;
            let choices = check_choices(&check_code);
            let shown = LinkCheckShown {
                choices: choices.clone(),
                platform: request.platform,
            };
            open.opened = Some(OpenedCheck {
                request,
                check_code,
                choices,
            });
            (Answer::Check(check).encode()?, shown)
        };
        self.bucket().put_link_slot(&token, "answer", answer.0).await?;
        Ok(Some(answer.1))
    }

    /// Drops the open code: the person chose "None of these", or closed
    /// the dialog.
    pub fn cancel_link(&self) {
        *self.open_link_slot() = None;
    }

    /// Links the device whose code the person picked: vouches for it
    /// (after catching up, so the certificate is fresh), asks the admin's
    /// app to add it, and hands it what it needs to read at once. Any
    /// other pick ends the link and spends the code.
    pub async fn confirm_link(&self, picked: &str) -> Result<()> {
        let Some(OpenLink {
            code,
            opened: Some(opened),
            ..
        }) = self.open_link_slot().take()
        else {
            bail!("there's no new device waiting to be confirmed");
        };
        ensure!(
            picked == opened.check_code,
            "That wasn't the number. Start again with a new code."
        );
        let request = opened.request;
        self.sync().await;
        let certificate = {
            let state = self.state();
            DeviceCertificate::vouch(
                &self.inner.identity,
                &self.inner.config.workspace_id,
                self.my_person(),
                request.device,
                state.mls.epoch()?,
                request.linked.acceptance.clone(),
            )?
        };
        let now = now_ms()?;
        let pending = PendingLink {
            answer_until_ms: code.expires_ms - LINK_CODE_LIFETIME_MS + ANSWER_LIFETIME_MS,
            code,
            request,
            certificate,
            is_handed_over: false,
            until_ms: now + PENDING_LINK_LIFETIME_MS,
        };
        self.state().store.put_pending_link(&pending)?;
        self.finish_pending_link(pending).await
    }

    /// Does what is left of a confirmed link: the request op, then the
    /// hand-over while the new device can still read it.
    async fn finish_pending_link(&self, mut pending: PendingLink) -> Result<()> {
        if !self.has_asked_to_link(&pending)? {
            self.write(Op::DeviceChange {
                change: DeviceChange::Link {
                    token: pending.code.token,
                    linked: pending.request.linked.clone(),
                    certificate: Box::new(pending.certificate.clone()),
                },
            })?;
        }
        if pending.is_handed_over || now_ms()? >= pending.answer_until_ms {
            return Ok(());
        }
        let handover = self.handover_for(&pending)?;
        let sealed = seal_handover(&self.inner.identity, &pending.code, &pending.request, &handover)?;
        self.bucket()
            .put_link_slot(&pending.code.token, "answer", Answer::Handover(sealed).encode()?)
            .await
            .context("handing the new device what it needs")?;
        pending.is_handed_over = true;
        self.state().store.put_pending_link(&pending)
    }

    fn has_asked_to_link(&self, pending: &PendingLink) -> Result<bool> {
        let me = self.me();
        Ok(self.state().store.device_changes()?.iter().any(|change| {
            change.author == me
                && matches!(&change.change, DeviceChange::Link { token, certificate, .. }
                    if *token == pending.code.token && **certificate == pending.certificate)
        }))
    }

    /// What a newcomer's grant holds, up to the certificate's epoch and no
    /// further.
    fn handover_for(&self, pending: &PendingLink) -> Result<Handover> {
        let epoch = pending.certificate.epoch;
        let state = self.state();
        let mut history = KeyHistory::collect(&state.store)?;
        history.epochs.retain(|entry| entry.epoch <= epoch);
        Ok(Handover {
            voucher: self.me(),
            device: pending.request.device,
            token: pending.code.token,
            epoch,
            history,
            devices: state.store.device_records()?,
            bound_frozen: state.store.bound_freezes()?,
            bucket: config::current_bucket(&state.store)?,
            bucket_lamport: config::bucket_lamport(&state.store)?,
        })
    }
}
