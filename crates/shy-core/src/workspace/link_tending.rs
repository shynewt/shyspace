//! What the device that confirmed a link does with it after each sync pass,
//! until the admin's app adds the new device or the record ends: finishes
//! what a restart left undone, vouches again for a certificate that went
//! stale, cancels a link whose hand-over never went up, and clears the
//! link's objects from the bucket once nobody needs them.

use anyhow::Result;
use tracing::warn;

use super::Workspace;
use crate::{
    membership::{
        device::DeviceCertificate,
        device_change::DeviceChange,
        handover::Answer,
        link_answers::{Reason, SignedOutcome},
    },
    ops::{Op, now_ms},
    storage::{PendingLink, RecordedChange},
};

impl Workspace {
    pub(crate) async fn tend_pending_links(&self) -> Result<()> {
        let _one_at_a_time = self.inner.pending_links_lock.lock().await;
        let pending = self.state().store.pending_links()?;
        for link in pending {
            if let Err(error) = self.tend_pending_link(link).await {
                warn!("a confirmed link is left for the next pass: {error:#}");
            }
        }
        Ok(())
    }

    async fn tend_pending_link(&self, mut link: PendingLink) -> Result<()> {
        let now = now_ms()?;
        let device = link.request.device;
        let token = link.code.token;
        if now >= link.until_ms {
            self.delete_link_slots(&link).await?;
            self.bucket()
                .delete_link_reply(&token, &device, "outcome")
                .await?;
            return self.state().store.delete_pending_link(&token);
        }
        // Before anything else: a device added without its hand-over can
        // never join, so it is taken back too.
        if !link.is_refused && !link.is_handed_over {
            return self.finish_or_cancel(link).await;
        }
        // Someone with the bucket keys may have taken the hand-over away
        // while the new device can still read it: it goes up again.
        if !link.is_refused && now < link.answer_until_ms && !self.is_handover_up(&link).await? {
            link.is_handed_over = false;
            return self.finish_pending_link(link).await;
        }
        if self.state().store.was_ever_member(&device)? {
            // The new device may not have fetched its hand-over yet: the slots
            // stay while the answer link works, then go with the record.
            if now < link.answer_until_ms {
                return Ok(());
            }
            self.delete_link_slots(&link).await?;
            self.bucket()
                .delete_link_reply(&token, &device, "outcome")
                .await?;
            return self.state().store.delete_pending_link(&token);
        }
        if !link.is_refused {
            match self.signed_outcome(&link).await? {
                Some(Reason::Stale) => return self.vouch_again(link).await,
                Some(_) => {
                    // A removal naming the device tells the person's other
                    // devices it isn't linking any more; the admin's app
                    // ignores it, as the link is already refused.
                    if !self.has_asked_to_remove(&device)? {
                        self.write(Op::DeviceChange {
                            change: DeviceChange::Remove {
                                devices: vec![device],
                            },
                        })?;
                    }
                    link.is_refused = true;
                    return self.state().store.put_pending_link(&link);
                }
                None => {}
            }
        }
        if now >= link.answer_until_ms && !link.are_slots_deleted {
            self.delete_link_slots(&link).await?;
            link.are_slots_deleted = true;
            self.state().store.put_pending_link(&link)?;
        }
        Ok(())
    }

    /// The admin's outcome for this link's current certificate, if it
    /// signed one. An outcome for an earlier certificate is passed over.
    async fn signed_outcome(&self, link: &PendingLink) -> Result<Option<Reason>> {
        let (token, device) = (&link.code.token, &link.request.device);
        let Some(bytes) = self.bucket().link_reply(token, device, "outcome").await? else {
            return Ok(None);
        };
        let config = &self.inner.config;
        let outcome = SignedOutcome::decode(&bytes).and_then(|signed| {
            Ok(signed
                .read(&config.admin, &config.workspace_id, token, device)?
                .clone())
        });
        match outcome {
            Ok(outcome) if outcome.certificate_hash == link.certificate.hash()? => Ok(Some(outcome.reason)),
            Ok(_) => Ok(None),
            Err(error) => {
                warn!("ignoring a link outcome the admin didn't sign: {error:#}");
                Ok(None)
            }
        }
    }

    /// Vouches for the same device again at the current epoch and asks
    /// again, unless the person has removed it since.
    async fn vouch_again(&self, mut link: PendingLink) -> Result<()> {
        if self.has_asked_to_remove(&link.request.device)? {
            link.is_refused = true;
            return self.state().store.put_pending_link(&link);
        }
        link.certificate = DeviceCertificate::vouch(
            &self.inner.identity,
            &self.inner.config.workspace_id,
            self.my_person(),
            link.request.device,
            self.state().mls.epoch()?,
            link.request.linked.acceptance.clone(),
        )?;
        self.state().store.put_pending_link(&link)?;
        self.finish_pending_link(link).await
    }

    /// Finishes a link a restart interrupted while the new device can still
    /// read its hand-over, and otherwise cancels it, or removes the device
    /// if it was added already, since it can never join.
    async fn finish_or_cancel(&self, link: PendingLink) -> Result<()> {
        if now_ms()? < link.answer_until_ms {
            return self.finish_pending_link(link).await;
        }
        let token = link.code.token;
        if !self.has_asked_to_link(&link)? {
            return self.state().store.delete_pending_link(&token);
        }
        if !self.has_asked_to_remove(&link.request.device)? {
            self.write(Op::DeviceChange {
                change: DeviceChange::Remove {
                    devices: vec![link.request.device],
                },
            })?;
        }
        let mut link = link;
        link.is_refused = true;
        self.state().store.put_pending_link(&link)
    }

    /// Whether the person still asks to remove `device`: a removal naming it
    /// by one of their devices that is still a member and not frozen.
    pub(super) fn has_asked_to_remove(&self, device: &iroh::PublicKey) -> Result<bool> {
        Ok(!self.live_removals_naming(device)?.is_empty())
    }

    /// The removals naming `device` that can still count: written by one of
    /// the person's devices that is a member now and not frozen. Members
    /// can't see which requests the admin's app skipped, so callers that
    /// show or skip a removal also look at the epoch it was written in.
    pub(super) fn live_removals_naming(&self, device: &iroh::PublicKey) -> Result<Vec<RecordedChange>> {
        let person = self.my_person();
        let state = self.state();
        let mut live = Vec::new();
        for change in state.store.device_changes()? {
            let names_it =
                matches!(&change.change, DeviceChange::Remove { devices } if devices.contains(device));
            if change.person == person
                && names_it
                && state.mls.is_member(&change.author)?
                && state.store.frozen_last_seq(&change.author)?.is_none()
            {
                live.push(change);
            }
        }
        Ok(live)
    }

    async fn is_handover_up(&self, link: &PendingLink) -> Result<bool> {
        let answer = self.bucket().link_slot(&link.code.token, "answer").await?;
        Ok(answer.is_some_and(|bytes| matches!(Answer::decode(&bytes), Ok(Answer::Handover(_)))))
    }

    async fn delete_link_slots(&self, link: &PendingLink) -> Result<()> {
        if link.are_slots_deleted {
            return Ok(());
        }
        let bucket = self.bucket();
        bucket.delete_link_slot(&link.code.token, "request").await?;
        bucket.delete_link_slot(&link.code.token, "answer").await
    }
}
