//! The rules of private channels and DMs: which opened private ops count,
//! which must wait for something still missing, and which never will. Every
//! verdict depends only on ops that came before in their authors' logs, so
//! all members reach the same one in any arrival order.

use anyhow::Result;
use iroh::PublicKey;

use super::{
    crypto::commitment,
    ids::{PrivateKind, is_creator_of, is_message_in, private_kind},
    ops::{MAX_INITIAL_MEMBERS, PrivateBody, PrivateOp},
};
use crate::{
    ops::{Op, OpBody, VerifiedOp, clean_channel_name, is_owned_by},
    storage::{Applied, Origin, Store, StoredKey},
    views::Change,
};

pub(crate) enum Verdict {
    Apply,
    Wait,
    Void(&'static str),
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
        if self.has_earlier_waiting(&body.channel, &verified.author, verified.seq)? {
            return Ok(Verdict::Wait);
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
        let author = &verified.author;
        if let PrivateOp::Create {
            commitment: announced,
            members,
            ..
        } = &body.op
        {
            return Ok(if !is_creator_of(channel, author) {
                Verdict::Void("only the creator its id names can start a channel")
            } else if key.generation != 1 || commitment(&key.key) != *announced {
                Verdict::Void("a channel must start with the key it announces")
            } else if members.len() > MAX_INITIAL_MEMBERS {
                Verdict::Void("too many first members")
            } else if self.commitment_of(channel, 1)?.is_some() && !self.has_origin(author, verified.seq)? {
                Verdict::Void("the channel was already started")
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
        // Applying the same op again, as a rebuild does, must change nothing.
        if self.has_origin(author, verified.seq)? {
            return Ok(Verdict::Apply);
        }
        match &body.op {
            PrivateOp::Create { .. } => Ok(Verdict::Void("the channel was already started")),
            PrivateOp::Add {
                from_generation,
                owner_index,
                ..
            } => self.judge_as_owner(verified, channel, *owner_index, || {
                let latest = self.latest_generation(channel)?;
                Ok(match from_generation.cmp(&latest) {
                    std::cmp::Ordering::Equal => Verdict::Apply,
                    std::cmp::Ordering::Greater => Verdict::Wait,
                    std::cmp::Ordering::Less => Verdict::Void("an add from an older key generation"),
                })
            }),
            PrivateOp::Remove {
                generation,
                owner_index,
                ..
            }
            | PrivateOp::Rotate {
                generation,
                owner_index,
                ..
            } => self.judge_as_owner(verified, channel, *owner_index, || {
                self.judge_next_generation(channel, *generation)
            }),
            PrivateOp::HandOver { to, owner_index } => {
                self.judge_as_owner(verified, channel, *owner_index, || {
                    Ok(
                        if self.private_owners(channel)?.len() > *owner_index as usize + 1 {
                            Verdict::Void("already handed over")
                        } else if !self.current_private_members(channel)?.contains(to) {
                            Verdict::Void("handed over to someone who isn't a member")
                        } else {
                            Verdict::Apply
                        },
                    )
                })
            }
            PrivateOp::Leave => self.judge_membership(verified, channel, key.generation),
            PrivateOp::Content(op) => match self.judge_membership(verified, channel, key.generation)? {
                Verdict::Apply => self.judge_content(channel, author, op, PrivateKind::Channel),
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
        match &body.op {
            PrivateOp::Content(op) => {
                self.judge_content(&body.channel, &verified.author, op, PrivateKind::Dm)
            }
            _ => Ok(Verdict::Void("DMs have no owners or membership changes")),
        }
    }

    /// Owner-only ops must come from the owner at `owner_index`, before they
    /// handed the channel over.
    fn judge_as_owner(
        &self,
        verified: &VerifiedOp,
        channel: &str,
        owner_index: u32,
        then: impl FnOnce() -> Result<Verdict>,
    ) -> Result<Verdict> {
        let owners = self.private_owners(channel)?;
        let index = owner_index as usize;
        match owners.get(index) {
            None => Ok(Verdict::Wait),
            Some(owner) if owner.member != verified.author => {
                Ok(Verdict::Void("only the channel's owner can do that"))
            }
            Some(_) => match owners.get(index + 1) {
                Some(next) if verified.seq > next.origin_seq => {
                    Ok(Verdict::Void("written after handing the channel over"))
                }
                _ => then(),
            },
        }
    }

    fn judge_next_generation(&self, channel: &str, generation: u64) -> Result<Verdict> {
        let next = self.latest_generation(channel)? + 1;
        Ok(match generation.cmp(&next) {
            std::cmp::Ordering::Equal => Verdict::Apply,
            std::cmp::Ordering::Greater => Verdict::Wait,
            std::cmp::Ordering::Less => Verdict::Void("a key generation that already exists"),
        })
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

    /// Content stays inside its channel: it names only the channel and its
    /// messages, and changes only what its author may change.
    fn judge_content(
        &self,
        channel: &str,
        author: &PublicKey,
        op: &Op,
        kind: PrivateKind,
    ) -> Result<Verdict> {
        let is_channel = kind == PrivateKind::Channel;
        let is_owner = || -> Result<bool> {
            Ok(self
                .private_owners(channel)?
                .last()
                .is_some_and(|owner| owner.member == *author))
        };
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
            Op::MessageEdit { id, .. } => is_message_in(id, channel) && is_owned_by(id, author),
            Op::Delete { id } if is_message_in(id, channel) => {
                is_owned_by(id, author) || (is_channel && is_owner()?)
            }
            Op::Delete { id } => is_channel && id == channel && is_owner()?,
            Op::ChannelRename { id, .. } => is_channel && id == channel,
            _ => false,
        };
        Ok(if is_inside {
            Verdict::Apply
        } else {
            Verdict::Void("content that reaches outside its channel")
        })
    }

    /// Applies an op the rules allowed.
    pub(crate) fn apply_private(&self, verified: &VerifiedOp, body: &PrivateBody) -> Result<Applied> {
        let channel = body.channel.as_str();
        let author = &verified.author;
        let origin = Origin {
            author,
            seq: verified.seq,
        };
        let mut applied = Applied::default();
        match &body.op {
            PrivateOp::Create {
                name,
                commitment,
                members,
            } => {
                self.record_private_channel(channel, PrivateKind::Channel, origin)?;
                self.append_private_owner(channel, 0, author, origin)?;
                self.record_generation(channel, 1, commitment, origin)?;
                self.record_add(channel, author, verified.seq - 1, 1, origin)?;
                for invitee in members {
                    self.record_add(channel, &invitee.member, invitee.after_seq, 1, origin)?;
                }
                self.show_channel(channel, &clean_channel_name(name), verified)?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::Add {
                invitee,
                from_generation,
                ..
            } => {
                self.record_add(
                    channel,
                    &invitee.member,
                    invitee.after_seq,
                    *from_generation,
                    origin,
                )?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::Remove {
                member,
                last_seq,
                generation,
                commitment,
                ..
            } => {
                self.record_closure(channel, member, *last_seq, *generation, origin)?;
                self.record_generation(channel, *generation, commitment, origin)?;
                applied.needs_rebuild = self.has_applied_after(channel, member, *last_seq)?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::Rotate {
                generation,
                commitment,
                ..
            } => self.record_generation(channel, *generation, commitment, origin)?,
            PrivateOp::HandOver { to, owner_index } => {
                self.append_private_owner(channel, owner_index + 1, to, origin)?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::Leave => {
                self.record_closure(channel, author, verified.seq, body.generation + 1, origin)?;
                applied.needs_rebuild = self.has_applied_after(channel, author, verified.seq)?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::Content(op) => {
                let content = VerifiedOp {
                    author: *author,
                    seq: verified.seq,
                    body: OpBody {
                        lamport: verified.body.lamport,
                        timestamp_ms: verified.body.timestamp_ms,
                        op: op.clone(),
                    },
                };
                if let Some(change) = self.apply(&content)? {
                    applied.changes.push(change);
                    applied.readable.push(content);
                }
            }
        }
        Ok(applied)
    }
}
