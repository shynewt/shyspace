//! Applying key grants and sealed ops. Keys from grants are only candidates
//! until the channel's owner vouches for them with a commitment; a sealed op
//! waits until its key is vouched for and its author belongs, then applies in
//! its author's order. Opening one op can let others apply, so every change
//! retries the ops still waiting.

use anyhow::{Result, bail};
use tracing::warn;

use super::{
    crypto::{Granting, SealedPayload, Slot, Wrap, key_hint, open, unwrap_key},
    ids::{MAX_DM_MEMBERS, PrivateKind, dm_id, private_kind},
    ops::{GrantBody, MAX_WRAPS_PER_GRANT, PrivateBody},
    rules::Verdict,
};
use crate::{
    ops::{Op, RuleViolation, VerifiedOp, rule_violation},
    storage::{Applied, ApplyContext, Origin, Store, StoredKey, WaitingSealed},
    views::Change,
};

enum Settled {
    Applied(Applied),
    Void,
    Waiting,
}

fn slot<'a>(verified: &'a VerifiedOp, context: ApplyContext<'a>) -> Slot<'a> {
    Slot {
        workspace_id: context.workspace_id,
        author: &verified.author,
        seq: verified.seq,
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
            self.match_new_keys(&new_keys, context)?;
            applied.extend(self.retry_sealed(context)?);
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
            Some(PrivateKind::Channel) if grant.dm_members.is_empty() && grant.generation >= 1 => {}
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

    /// Notes which channel each unopened sealed op belongs to, for the keys
    /// that just arrived.
    fn match_new_keys(&self, new_keys: &[StoredKey], context: ApplyContext<'_>) -> Result<()> {
        for waiting in self.waiting_sealed(false)? {
            let slot = Slot {
                workspace_id: context.workspace_id,
                author: &waiting.author,
                seq: waiting.seq,
            };
            for key in new_keys {
                if key_hint(&key.key, slot)? == waiting.hint {
                    self.match_waiting_sealed(&waiting.author, waiting.seq, &key.channel)?;
                    break;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn apply_sealed(
        &self,
        verified: &VerifiedOp,
        payload: &SealedPayload,
        context: ApplyContext<'_>,
    ) -> Result<Applied> {
        let slot = slot(verified, context);
        let mut channel = None;
        for key in self.channel_keys(None)? {
            if key_hint(&key.key, slot)? == payload.hint {
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
                applied.extend(self.retry_sealed(context)?);
                Ok(applied)
            }
            Settled::Void | Settled::Waiting => Ok(Applied::default()),
        }
    }

    /// Tries every matched waiting op until none moves.
    pub(crate) fn retry_sealed(&self, context: ApplyContext<'_>) -> Result<Applied> {
        let mut applied = Applied::default();
        loop {
            let mut has_progress = false;
            for waiting in self.waiting_sealed(true)? {
                let Some(channel) = &waiting.channel else {
                    continue;
                };
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
        let slot = slot(verified, context);
        for key in self.channel_keys(Some(channel))? {
            if key_hint(&key.key, slot)? != payload.hint {
                continue;
            }
            let Ok(plaintext) = open(&key.key, slot, payload) else {
                continue;
            };
            let Ok(body) = postcard::from_bytes::<PrivateBody>(&plaintext) else {
                // A kind of private op a newer version added.
                return Ok(Settled::Waiting);
            };
            return match self.judge(verified, &body, &key)? {
                Verdict::Wait => Ok(Settled::Waiting),
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
