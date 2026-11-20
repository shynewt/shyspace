//! The rules of private channels and DMs: which opened private ops count,
//! which must wait for something still missing, and which never will.
//!
//! Owner ops form one numbered log per channel, applied strictly in order,
//! so each is judged against the same state on every device (if an owner
//! writes two ops for one position, the earlier in their log wins).
//! Everything else is judged by who belonged when, which only ever shrinks as removals and
//! leaves arrive; content that turns out not to count is undone by a rebuild.

use anyhow::Result;
use iroh::PublicKey;

use super::{
    crypto::commitment,
    ids::{PrivateKind, is_creator_of, is_message_in, private_channel_id, private_kind},
    ops::{MAX_INITIAL_MEMBERS, PrivateBody, PrivateOp},
};
use crate::{
    ops::{Op, VerifiedOp, is_owned_by},
    storage::{Store, StoredKey},
};

pub(crate) enum Verdict {
    Apply,
    Wait,
    Void(&'static str),
    /// An owner op earlier in its author's log than the one that took its
    /// position: it should have won.
    Supersede,
}

impl Store {
    /// Whether `body`, opened from `verified` with `key`, may apply now.
    pub(crate) fn judge(
        &self,
        verified: &VerifiedOp,
        body: &PrivateBody,
        key: &StoredKey,
    ) -> Result<Verdict> {
        if body.channel != key.channel || body.generation != key.generation {
            return Ok(Verdict::Void("sealed for another channel or key"));
        }
        match private_kind(&body.channel) {
            Some(PrivateKind::Channel) => self.judge_in_channel(verified, body, key),
            Some(PrivateKind::Dm) => self.judge_in_dm(verified, body),
            None => Ok(Verdict::Void("not a private channel")),
        }
    }

    fn judge_in_channel(
        &self,
        verified: &VerifiedOp,
        body: &PrivateBody,
        key: &StoredKey,
    ) -> Result<Verdict> {
        let channel = body.channel.as_str();
        if let PrivateOp::Create {
            name,
            commitment: announced,
            members,
        } = &body.op
        {
            return Ok(if self.next_position(channel)? != 1 {
                Verdict::Void("the channel was already started")
            } else if key.generation != 1 || commitment(&key.key) != *announced {
                Verdict::Void("a channel must start with the key it announces")
            } else if members.len() > MAX_INITIAL_MEMBERS {
                Verdict::Void("too many first members")
            } else if !is_creator_of(channel, &verified.author)
                || private_channel_id(&verified.author, announced, name, members)? != channel
            {
                Verdict::Void("a creation that doesn't match the channel's id")
            } else {
                Verdict::Apply
            });
        }
        match self.commitment_of(channel, key.generation)? {
            None => return Ok(Verdict::Wait),
            Some(announced) if announced != commitment(&key.key) => {
                return Ok(Verdict::Void("a key the channel's owner didn't announce"));
            }
            Some(_) => {}
        }
        match &body.op {
            PrivateOp::Create { .. } => Ok(Verdict::Void("the channel was already started")),
            PrivateOp::Add {
                invitee,
                from_generation,
                position,
            } => self.judge_owner_op(verified, channel, *position, || {
                Ok(if *from_generation != self.latest_generation(channel)? {
                    Verdict::Void("an add from another key generation")
                } else if self.owner_membership(channel, &invitee.member)?.is_some() {
                    Verdict::Void("they are already in the channel")
                } else {
                    Verdict::Apply
                })
            }),
            PrivateOp::Remove {
                member,
                generation,
                position,
                ..
            } => self.judge_owner_op(verified, channel, *position, || {
                Ok(if *generation != self.latest_generation(channel)? + 1 {
                    Verdict::Void("not the next key generation")
                } else if self.owner_membership(channel, member)?.is_none() {
                    Verdict::Void("they aren't in the channel")
                } else {
                    Verdict::Apply
                })
            }),
            PrivateOp::Rotate {
                generation, position, ..
            } => self.judge_owner_op(verified, channel, *position, || {
                Ok(if *generation == self.latest_generation(channel)? + 1 {
                    Verdict::Apply
                } else {
                    Verdict::Void("not the next key generation")
                })
            }),
            PrivateOp::HandOver { to, position } => self.judge_owner_op(verified, channel, *position, || {
                Ok(
                    if *to == verified.author || self.owner_membership(channel, to)?.is_none() {
                        Verdict::Void("handed over to someone who isn't in the channel")
                    } else {
                        Verdict::Apply
                    },
                )
            }),
            PrivateOp::OwnerDelete { id, position } => {
                self.judge_owner_op(verified, channel, *position, || {
                    Ok(if id == channel || is_message_in(id, channel) {
                        Verdict::Apply
                    } else {
                        Verdict::Void("a delete outside the channel")
                    })
                })
            }
            PrivateOp::Leave => self.judge_membership(verified, channel, key.generation),
            PrivateOp::Content(op) => match self.judge_membership(verified, channel, key.generation)? {
                Verdict::Apply => Ok(judge_content(channel, &verified.author, op, PrivateKind::Channel)),
                other => Ok(other),
            },
        }
    }

    fn judge_in_dm(&self, verified: &VerifiedOp, body: &PrivateBody) -> Result<Verdict> {
        if body.generation != 0 {
            return Ok(Verdict::Void("DM keys have no generations"));
        }
        if !self.dm_members(&body.channel)?.contains(&verified.author) {
            return Ok(Verdict::Void("written by someone outside the DM"));
        }
        Ok(match &body.op {
            PrivateOp::Content(op) => judge_content(&body.channel, &verified.author, op, PrivateKind::Dm),
            _ => Verdict::Void("DMs have no owners or membership changes"),
        })
    }

    /// An owner op counts only as the channel's next position, written by
    /// whoever owns that position.
    fn judge_owner_op(
        &self,
        verified: &VerifiedOp,
        channel: &str,
        position: u64,
        then: impl FnOnce() -> Result<Verdict>,
    ) -> Result<Verdict> {
        let next = self.next_position(channel)?;
        if position > next {
            return Ok(Verdict::Wait);
        }
        if position < next {
            let taken_by = self.position_origin(channel, position)?;
            return Ok(match taken_by {
                Some((author, seq)) if author == verified.author && seq > verified.seq => Verdict::Supersede,
                _ => Verdict::Void("a position already taken"),
            });
        }
        if self.owner_at(channel, position)? != Some(verified.author) {
            return Ok(Verdict::Void("only the channel's owner can do that"));
        }
        then()
    }

    /// An op counts only while its author belongs. One sealed with a key
    /// older than the newest can never come to count: any later add starts at
    /// the newest key.
    fn judge_membership(&self, verified: &VerifiedOp, channel: &str, generation: u64) -> Result<Verdict> {
        Ok(
            if self.is_covered(channel, &verified.author, verified.seq, generation)? {
                Verdict::Apply
            } else if generation < self.latest_generation(channel)? {
                Verdict::Void("written by someone who isn't a member")
            } else {
                Verdict::Wait
            },
        )
    }
}

/// Content stays inside its channel: it names only the channel and its
/// messages, and changes only what its author may change.
fn judge_content(channel: &str, author: &PublicKey, op: &Op, kind: PrivateKind) -> Verdict {
    let is_inside = match op {
        Op::Message {
            id,
            channel: target,
            reply_to,
            ..
        } => {
            target == channel
                && is_message_in(id, channel)
                && is_owned_by(id, author)
                && reply_to
                    .as_deref()
                    .is_none_or(|parent| is_message_in(parent, channel))
        }
        Op::Reaction { message, .. } => is_message_in(message, channel),
        Op::MessageEdit { id, .. } | Op::Delete { id } => {
            is_message_in(id, channel) && is_owned_by(id, author)
        }
        Op::ChannelRename { id, .. } => kind == PrivateKind::Channel && id == channel,
        _ => false,
    };
    if is_inside {
        Verdict::Apply
    } else {
        Verdict::Void("content that reaches outside its channel")
    }
}
