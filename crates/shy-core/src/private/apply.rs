//! Applying key grants and sealed ops. A sealed op waits until we hold its
//! key, the channel's owner has vouched for that key, and the channel's rules
//! let it count. Each change retries the ops still waiting in that channel.

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use tracing::warn;

use super::{
    crypto::{Granting, SealedPayload, Slot, Wrap, commitment, key_hint, open, unwrap_key},
    ids::{MAX_DM_MEMBERS, PrivateKind, dm_id, private_kind},
    ops::{GrantBody, MAX_WRAPS_PER_GRANT, PrivateBody, PrivateOp},
    rules::Verdict,
};
use crate::{
    ops::{Op, OpBody, RuleViolation, VerifiedOp, clean_channel_name, rule_violation},
    storage::{Applied, ApplyContext, Ending, Membership, Origin, Settling, Store, StoredKey, WaitingSealed},
    views::Change,
};

enum Settled {
    Applied(Applied),
    Void,
    Waiting,
}

fn slot<'a>(author: &'a iroh::PublicKey, seq: u64, context: ApplyContext<'a>) -> Slot<'a> {
    Slot {
        workspace_id: context.workspace_id,
        author,
        seq,
    }
}

impl Store {
    pub(crate) fn apply_key_grant(
        &self,
        verified: &VerifiedOp,
        wraps: &[Wrap],
        context: ApplyContext<'_>,
    ) -> Result<Applied> {
        if wraps.len() > MAX_WRAPS_PER_GRANT {
            return Err(rule_violation("a key grant with too many wraps"));
        }
        let granting = Granting {
            workspace_id: context.workspace_id,
            granter: &verified.author,
        };
        let secrets = self.sealing_secrets()?;
        let mut applied = Applied::default();
        let mut new_keys = Vec::new();
        for wrap in wraps {
            let Some(plaintext) = secrets
                .iter()
                .find_map(|secret| unwrap_key(granting, secret, wrap).ok())
            else {
                continue;
            };
            match postcard::from_bytes::<GrantBody>(&plaintext) {
                Ok(grant) => {
                    if let Some(key) = self.accept_grant(grant, verified, context, &mut applied)? {
                        new_keys.push(key);
                    }
                }
                Err(error) => warn!("ignoring an unreadable key grant: {error}"),
            }
        }
        if !new_keys.is_empty() {
            let channels = self.match_new_keys(&new_keys, context)?;
            applied.extend(self.retry_sealed(&channels, context)?);
        }
        Ok(applied)
    }

    /// Keeps a granted key; returns it if it's new. A DM's key also makes the
    /// DM, once its member list checks out.
    fn accept_grant(
        &self,
        grant: GrantBody,
        verified: &VerifiedOp,
        context: ApplyContext<'_>,
        applied: &mut Applied,
    ) -> Result<Option<StoredKey>> {
        let origin = Origin {
            author: &verified.author,
            seq: verified.seq,
        };
        match private_kind(&grant.channel) {
            Some(PrivateKind::Channel) if grant.dm_members.is_empty() && grant.generation >= 1 => {
                let announced = self.commitment_of(&grant.channel, grant.generation)?;
                if announced.is_some_and(|announced| announced != commitment(&grant.key)) {
                    warn!("ignoring a key its channel's owner didn't announce");
                    return Ok(None);
                }
            }
            Some(PrivateKind::Dm) if is_valid_dm_grant(&grant, verified, context) => {
                self.record_private_channel(&grant.channel, PrivateKind::Dm, origin)?;
                self.put_dm_members(&grant.channel, &grant.dm_members)?;
                if self.show_channel(&grant.channel, "", verified)? {
                    applied.changes.push(Change::Channels);
                }
            }
            _ => {
                warn!("ignoring a malformed key grant");
                return Ok(None);
            }
        }
        let key = StoredKey {
            channel: grant.channel,
            generation: grant.generation,
            key: grant.key,
        };
        Ok(self.put_channel_key(&key, origin)?.then_some(key))
    }

    /// Notes which channel each unmatched sealed op belongs to, for the keys
    /// that just arrived. Returns the channels that gained keys.
    fn match_new_keys(&self, new_keys: &[StoredKey], context: ApplyContext<'_>) -> Result<BTreeSet<String>> {
        for waiting in self.unmatched_sealed()? {
            let slot = slot(&waiting.author, waiting.seq, context);
            for key in new_keys {
                if key_hint(key.labeled(), slot)? == waiting.hint {
                    self.match_waiting_sealed(&waiting.author, waiting.seq, &key.channel)?;
                    break;
                }
            }
        }
        Ok(new_keys.iter().map(|key| key.channel.clone()).collect())
    }

    pub(crate) fn apply_sealed(
        &self,
        verified: &VerifiedOp,
        payload: &SealedPayload,
        context: ApplyContext<'_>,
    ) -> Result<Applied> {
        let slot = slot(&verified.author, verified.seq, context);
        let mut channel = None;
        for key in self.channel_keys(None)? {
            if key_hint(key.labeled(), slot)? == payload.hint {
                channel = Some(key.channel);
                break;
            }
        }
        self.put_waiting_sealed(&WaitingSealed {
            author: verified.author,
            seq: verified.seq,
            hint: payload.hint,
            channel: channel.clone(),
        })?;
        let Some(channel) = channel else {
            return Ok(Applied::default());
        };
        match self.settle_sealed(verified, payload, &channel, context)? {
            Settled::Applied(mut applied) => {
                applied.extend(self.retry_sealed(&BTreeSet::from([channel]), context)?);
                Ok(applied)
            }
            Settled::Void | Settled::Waiting => Ok(Applied::default()),
        }
    }

    /// Tries the waiting ops of `channels` until none moves.
    pub(crate) fn retry_sealed(
        &self,
        channels: &BTreeSet<String>,
        context: ApplyContext<'_>,
    ) -> Result<Applied> {
        let mut applied = Applied::default();
        loop {
            let mut has_progress = false;
            for channel in channels {
                for waiting in self.waiting_in(channel)? {
                    let verified = self.stored_op(&waiting.author, waiting.seq)?;
                    let Op::Sealed { payload } = &verified.body.op else {
                        bail!("a waiting sealed op isn't sealed");
                    };
                    match self.settle_sealed(&verified, payload, channel, context)? {
                        Settled::Applied(more) => {
                            applied.extend(more);
                            has_progress = true;
                        }
                        Settled::Void => has_progress = true,
                        Settled::Waiting => {}
                    }
                }
            }
            if !has_progress {
                return Ok(applied);
            }
        }
    }

    /// Opens a waiting op with the channel's keys and applies it if the
    /// channel's rules allow, drops it if they never will, or leaves it.
    fn settle_sealed(
        &self,
        verified: &VerifiedOp,
        payload: &SealedPayload,
        channel: &str,
        context: ApplyContext<'_>,
    ) -> Result<Settled> {
        let slot = slot(&verified.author, verified.seq, context);
        for key in self.channel_keys(Some(channel))? {
            if key_hint(key.labeled(), slot)? != payload.hint {
                continue;
            }
            let Ok(plaintext) = open(key.labeled(), slot, payload) else {
                continue;
            };
            let Ok(body) = postcard::from_bytes::<PrivateBody>(&plaintext) else {
                // A kind of private op a newer version added.
                return Ok(Settled::Waiting);
            };
            if !may_settle_now(self.settling.get(), &body.op) {
                return Ok(Settled::Waiting);
            }
            return match self.judge(verified, &body, &key)? {
                Verdict::Wait => Ok(Settled::Waiting),
                Verdict::Supersede => {
                    // An earlier op of the owner's for a position another
                    // already took: it wins, and the views start over.
                    self.delete_waiting_sealed(&verified.author, verified.seq)?;
                    Ok(Settled::Applied(Applied {
                        needs_rebuild: true,
                        ..Applied::default()
                    }))
                }
                Verdict::Void(reason) => {
                    warn!("skipping a private op: {reason}");
                    self.delete_waiting_sealed(&verified.author, verified.seq)?;
                    Ok(Settled::Void)
                }
                Verdict::Apply => {
                    self.delete_waiting_sealed(&verified.author, verified.seq)?;
                    self.apply_judged(verified, &body)
                }
            };
        }
        warn!("skipping a sealed op that none of the channel's keys opens");
        self.delete_waiting_sealed(&verified.author, verified.seq)?;
        Ok(Settled::Void)
    }

    fn apply_judged(&self, verified: &VerifiedOp, body: &PrivateBody) -> Result<Settled> {
        match self.with_savepoint(|store| store.apply_private(verified, body)) {
            Ok(applied) => {
                self.record_private_applied(&body.channel, &verified.author, verified.seq)?;
                Ok(Settled::Applied(applied))
            }
            Err(error) if error.downcast_ref::<RuleViolation>().is_some() => {
                warn!("skipping a private op that breaks a rule: {error:#}");
                Ok(Settled::Void)
            }
            Err(error) => Err(error),
        }
    }

    /// Applies an op the rules allowed.
    fn apply_private(&self, verified: &VerifiedOp, body: &PrivateBody) -> Result<Applied> {
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
                self.record_position(channel, 1, origin)?;
                self.set_owner_from(channel, 1, author, origin)?;
                self.record_generation(channel, 1, commitment, origin)?;
                self.drop_unannounced_keys(channel, 1, commitment)?;
                let creator = Membership {
                    after_seq: verified.seq - 1,
                    from_generation: 1,
                };
                self.record_add(channel, author, creator, origin)?;
                for invitee in members {
                    let membership = Membership {
                        after_seq: invitee.after_seq,
                        from_generation: 1,
                    };
                    self.record_add(channel, &invitee.member, membership, origin)?;
                }
                self.show_channel(channel, &clean_channel_name(name), verified)?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::Add {
                invitee,
                from_generation,
                position,
            } => {
                self.record_position(channel, *position, origin)?;
                let membership = Membership {
                    after_seq: invitee.after_seq,
                    from_generation: *from_generation,
                };
                self.record_add(channel, &invitee.member, membership, origin)?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::Remove {
                member,
                last_seq,
                generation,
                commitment,
                position,
            } => {
                // Ends their latest time even if the owner hadn't seen that
                // far into their log.
                let started_after = self
                    .owner_membership(channel, member)?
                    .map_or(0, |membership| membership.after_seq);
                let at_seq = (*last_seq).max(started_after);
                self.record_position(channel, *position, origin)?;
                self.record_closure(
                    channel,
                    member,
                    at_seq,
                    *generation,
                    Ending::RemovedByOwner,
                    origin,
                )?;
                self.record_generation(channel, *generation, commitment, origin)?;
                self.drop_unannounced_keys(channel, *generation, commitment)?;
                applied.needs_rebuild = self.has_applied_after(channel, member, at_seq)?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::Rotate {
                generation,
                commitment,
                position,
            } => {
                self.record_position(channel, *position, origin)?;
                self.record_generation(channel, *generation, commitment, origin)?;
                self.drop_unannounced_keys(channel, *generation, commitment)?;
            }
            PrivateOp::HandOver { to, position } => {
                self.record_position(channel, *position, origin)?;
                self.set_owner_from(channel, position + 1, to, origin)?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::OwnerDelete { id, position } => {
                self.record_position(channel, *position, origin)?;
                applied.extend(self.apply_content(verified, Op::Delete { id: id.clone() })?);
            }
            PrivateOp::Leave => {
                self.record_closure(
                    channel,
                    author,
                    verified.seq,
                    body.generation + 1,
                    Ending::Left,
                    origin,
                )?;
                applied.needs_rebuild = self.has_applied_after(channel, author, verified.seq)?;
                applied.changes.push(Change::Channels);
            }
            PrivateOp::Content(op) => applied.extend(self.apply_content(verified, op.clone())?),
        }
        Ok(applied)
    }

    /// Applies content from a sealed op as if its author had written it plainly.
    fn apply_content(&self, verified: &VerifiedOp, op: Op) -> Result<Applied> {
        let content = VerifiedOp {
            author: verified.author,
            seq: verified.seq,
            body: OpBody {
                lamport: verified.body.lamport,
                timestamp_ms: verified.body.timestamp_ms,
                op,
            },
        };
        Ok(match self.apply(&content)? {
            Some(change) => Applied {
                changes: vec![change],
                readable: vec![content],
                needs_rebuild: false,
            },
            None => Applied::default(),
        })
    }
}

/// While a rebuild settles in phases, only some kinds of private op apply.
fn may_settle_now(settling: Settling, op: &PrivateOp) -> bool {
    match settling {
        Settling::Nothing => false,
        Settling::OwnerOps => !matches!(op, PrivateOp::Leave | PrivateOp::Content(_)),
        Settling::Leaves => !matches!(op, PrivateOp::Content(_)),
        Settling::Everything => true,
    }
}

/// A DM's key comes from one of its members, for another, and its id is the
/// hash of exactly that member list.
fn is_valid_dm_grant(grant: &GrantBody, verified: &VerifiedOp, context: ApplyContext<'_>) -> bool {
    let mut unique = grant.dm_members.clone();
    unique.sort_unstable_by_key(|member| *member.as_bytes());
    unique.dedup();
    grant.generation == 0
        && unique.len() == grant.dm_members.len()
        && (2..=MAX_DM_MEMBERS).contains(&unique.len())
        && unique.contains(&verified.author)
        && unique.contains(&context.me)
        && dm_id(context.workspace_id, &unique) == grant.channel
}
