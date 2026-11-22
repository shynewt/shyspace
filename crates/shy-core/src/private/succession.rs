//! Owner succession. When a private channel's owner is removed from the
//! workspace, the member they added earliest who is still around takes over
//! with a `Succeed` op at the channel's next position. Every device judges
//! that op from facts that, once known, never change, and waits until it
//! knows them all:
//! - the owner's removal, and their whole log up to it, so nothing of theirs
//!   can still take the position;
//! - every channel key announced so far, so no owner op sits unread;
//! - for each member ranked earlier, their removal too, so they can't still
//!   claim first.
//!
//! A claim must be written after the owner left the workspace, so nobody can
//! plant one in advance.

use anyhow::{Result, bail};
use iroh::PublicKey;

use super::{
    crypto::{Slot, key_hint, open},
    ops::{PrivateBody, PrivateOp},
    rules::Verdict,
};
use crate::{
    ops::Op,
    storage::{ApplyContext, Store},
};

impl Store {
    /// The verdict on `claimant` taking over `channel` at `position`, the
    /// channel's next one, with an op sealed in membership `epoch`.
    pub(crate) fn judge_succession(
        &self,
        channel: &str,
        position: u64,
        claimant: &PublicKey,
        epoch: u64,
        context: ApplyContext<'_>,
    ) -> Result<Verdict> {
        let Some(owner) = self.owner_at(channel, position)? else {
            return Ok(Verdict::Void("a channel without an owner"));
        };
        if owner == *claimant {
            return Ok(Verdict::Void("already the owner"));
        }
        if !self.is_removal_settled(&owner)? {
            return Ok(Verdict::Wait);
        }
        // Every device that applied the owner's ops knows their epochs.
        let Some(last_owner_epoch) = self.last_member_epoch(&owner)? else {
            return Ok(Verdict::Wait);
        };
        if epoch <= last_owner_epoch {
            return Ok(Verdict::Void("a claim written before the owner was removed"));
        }
        if !self.holds_keys_announced_before(channel, position)?
            || self.has_waiting_owner_op(channel, &owner, position, context, |_| true)?
        {
            return Ok(Verdict::Wait);
        }
        let candidates: Vec<PublicKey> = self
            .members_in_owner_order(channel)?
            .into_iter()
            .filter(|member| *member != owner)
            .collect();
        let Some(rank) = candidates.iter().position(|member| member == claimant) else {
            return Ok(Verdict::Void("a claim from someone not in the channel"));
        };
        for earlier in &candidates[..rank] {
            let may_still_claim = !self.is_removal_settled(earlier)?
                || self.has_waiting_owner_op(channel, earlier, position, context, |op| {
                    matches!(op, PrivateOp::Succeed { .. })
                })?;
            if may_still_claim {
                return Ok(Verdict::Wait);
            }
        }
        Ok(Verdict::Apply)
    }

    /// Whether `member` was removed from the workspace and their log up to
    /// the removal has all arrived.
    fn is_removal_settled(&self, member: &PublicKey) -> Result<bool> {
        let Some(cut) = self.frozen_last_seq(member)? else {
            return Ok(false);
        };
        Ok(self.last_seq(member)? >= cut)
    }

    /// Whether `author` has an op still waiting in `channel` that would take
    /// `position` and that `kind` picks, under a key announced before it.
    fn has_waiting_owner_op(
        &self,
        channel: &str,
        author: &PublicKey,
        position: u64,
        context: ApplyContext<'_>,
        kind: impl Fn(&PrivateOp) -> bool,
    ) -> Result<bool> {
        for waiting in self.waiting_in(channel)? {
            if waiting.author != *author {
                continue;
            }
            let Some(body) = self.open_waiting(channel, author, waiting.seq, context)? else {
                continue;
            };
            let is_announced_before = self
                .generation_position(channel, body.generation)?
                .is_some_and(|announced_at| announced_at < position);
            if body.op.position() == Some(position) && kind(&body.op) && is_announced_before {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// A waiting op of `channel`, opened with its keys, if one of them opens
    /// it and it's a kind this version knows.
    fn open_waiting(
        &self,
        channel: &str,
        author: &PublicKey,
        seq: u64,
        context: ApplyContext<'_>,
    ) -> Result<Option<PrivateBody>> {
        let Op::Sealed { payload } = self.stored_op(author, seq)?.body.op else {
            bail!("a waiting sealed op isn't sealed");
        };
        let slot = Slot {
            workspace_id: context.workspace_id,
            author,
            seq,
        };
        for key in self.channel_keys(Some(channel))? {
            if key_hint(key.labeled(), slot)? != payload.hint {
                continue;
            }
            if let Ok(plaintext) = open(key.labeled(), slot, &payload) {
                // One with impossible numbers is void wherever it's judged.
                return Ok(postcard::from_bytes::<PrivateBody>(&plaintext)
                    .ok()
                    .filter(PrivateBody::has_storable_numbers));
            }
        }
        Ok(None)
    }
}
