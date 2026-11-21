//! Accepting ops from others. An op is applied only if it decrypts, its
//! signature is valid, and its author was a member in the op's epoch.

use anyhow::{Result, bail, ensure};
use tracing::warn;

use super::forks::SecondLook;
use crate::{
    crypto::{self, EpochKey, NewerFormat},
    ops::{Envelope, MAX_PAGE_UPDATE_BYTES, RuleViolation, VerifiedOp},
    storage::{Applied, ApplyContext},
    views::Change,
    workspace::{State, Workspace},
};

/// Ops for epochs further ahead than this are dropped instead of parked, so
/// junk can't pile up waiting for a key that will never come.
pub(super) const MAX_PENDING_EPOCHS_AHEAD: u64 = 16;
/// SQLite integers are signed 64-bit.
const MAX_STORED_NUMBER: u64 = i64::MAX as u64;

impl Workspace {
    /// Takes an envelope from gossip or the bucket. It is stored only once it
    /// verifies; envelopes we can't apply yet are parked and retried.
    pub(crate) fn receive(&self, envelope: Envelope) -> Result<()> {
        ensure!(
            (1..=MAX_STORED_NUMBER).contains(&envelope.seq) && envelope.epoch <= MAX_STORED_NUMBER,
            "op numbering out of range"
        );
        let changes = {
            let state = self.state();
            if state.store.is_applied(&envelope.author, envelope.seq)? {
                match self.check_for_fork(&state, &envelope)? {
                    SecondLook::Settled(changes) => changes,
                    SecondLook::NotYet => {
                        park(&state, &envelope)?;
                        Vec::new()
                    }
                    SecondLook::Refused(reason) => bail!(reason),
                }
            } else {
                self.receive_new(&state, &envelope)?
            }
        };
        for change in changes {
            self.emit(change);
        }
        Ok(())
    }

    /// An op for a place in its author's log we haven't filled yet.
    fn receive_new(&self, state: &State, envelope: &Envelope) -> Result<Vec<Change>> {
        Ok(match verify_or_wait(state, envelope)? {
            Some(verified) if is_ready(state, &verified)? => {
                self.apply_with_alert(state, envelope, &verified)?
            }
            // Not kept while it waits: gap repair reads it again once a
            // later op of its author's waits behind it.
            Some(verified) if !envelope.fits_size_limit(&verified.body.op) => {
                warn!("not keeping an oversized op that arrived before its turn");
                Vec::new()
            }
            _ => {
                park(state, envelope)?;
                Vec::new()
            }
        })
    }

    fn apply_with_alert(
        &self,
        state: &State,
        envelope: &Envelope,
        verified: &VerifiedOp,
    ) -> Result<Vec<Change>> {
        let applied = apply_and_record(state, envelope, verified, self.apply_context())?;
        let mut changes = applied.changes;
        for readable in &applied.readable {
            changes.extend(self.alert_for(state, readable)?);
        }
        if applied.needs_rebuild {
            changes.extend(state.store.all_view_changes()?);
        }
        Ok(changes)
    }

    /// Retries parked ops, for example after a new epoch key arrived.
    /// Returns whether an op from a newer version is holding up its author.
    pub(super) fn apply_pending(&self) -> Result<bool> {
        let mut is_waiting_for_update = false;
        let changes = {
            let state = self.state();
            let mut changes = Vec::new();
            for envelope in state.store.pending()? {
                if state.store.is_applied(&envelope.author, envelope.seq)? {
                    match self.check_for_fork(&state, &envelope)? {
                        SecondLook::Settled(more) => changes.extend(more),
                        SecondLook::NotYet => continue,
                        SecondLook::Refused(reason) => warn!("dropping a parked op: {reason}"),
                    }
                    state.store.delete_pending(&envelope)?;
                    continue;
                }
                match verify(&state, &envelope) {
                    Ok(Some(verified)) if is_ready(&state, &verified)? => {
                        changes.extend(self.apply_with_alert(&state, &envelope, &verified)?);
                    }
                    Ok(_) => {}
                    Err(error) if error.downcast_ref::<NewerFormat>().is_some() => {
                        // Only the op its author needs next blocks anything;
                        // a far-ahead one can't raise a false alarm.
                        is_waiting_for_update |= envelope.seq == state.store.next_seq(&envelope.author)?;
                    }
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
        Ok(is_waiting_for_update)
    }
}

/// Like `verify`, but an op from a newer version also waits (`Ok(None)`).
fn verify_or_wait(state: &State, envelope: &Envelope) -> Result<Option<VerifiedOp>> {
    match verify(state, envelope) {
        Err(error) if error.downcast_ref::<NewerFormat>().is_some() => {
            warn!("keeping an op from a newer version of shyspace until this one is updated");
            Ok(None)
        }
        other => other,
    }
}

/// `Ok(None)` when we lack the epoch key yet.
fn verify(state: &State, envelope: &Envelope) -> Result<Option<VerifiedOp>> {
    match author_epoch_key(state, envelope)? {
        AuthorKey::NotYet => Ok(None),
        AuthorKey::Refused(reason) => bail!(reason),
        AuthorKey::Ready(key) => Ok(Some(crypto::open(envelope, &key)?)),
    }
}

pub(super) enum AuthorKey {
    Ready(EpochKey),
    /// The epoch key hasn't arrived yet.
    NotYet,
    /// Its author couldn't write it: not a member in its epoch, or past
    /// their frozen log.
    Refused(String),
}

/// The key to open `envelope` with, once we know its author could write it.
pub(super) fn author_epoch_key(state: &State, envelope: &Envelope) -> Result<AuthorKey> {
    let Some(key) = state.store.epoch_key(envelope.epoch)? else {
        return Ok(AuthorKey::NotYet);
    };
    if !state.store.was_member_at(envelope.epoch, &envelope.author)? {
        return Ok(AuthorKey::Refused(format!(
            "author was not a member in epoch {}",
            envelope.epoch
        )));
    }
    if state.store.is_beyond_frozen_log(&envelope.author, envelope.seq)? {
        return Ok(AuthorKey::Refused(
            "op written after its author was removed".into(),
        ));
    }
    Ok(AuthorKey::Ready(key))
}

/// An op applies only right after its author's previous one, so every
/// author's log has no gaps. That makes a removed member's frozen log exact:
/// nothing can be slipped in below the freeze. It also waits while its clock
/// is implausibly far ahead of ours.
fn is_ready(state: &State, verified: &VerifiedOp) -> Result<bool> {
    let follows_previous = verified.seq == 1 || state.store.is_applied(&verified.author, verified.seq - 1)?;
    Ok(follows_previous && state.store.accepts_clock(verified.body.lamport)?)
}

/// Applies a verified op. One that breaks a rule is recorded as handled
/// anyway, so it never blocks the author's later ops; one too large for its
/// kind is recorded without its content, so it takes no room.
fn apply_and_record(
    state: &State,
    envelope: &Envelope,
    verified: &VerifiedOp,
    context: ApplyContext<'_>,
) -> Result<Applied> {
    if !envelope.fits_size_limit(&verified.body.op) {
        warn!(
            "skipping an op too large for its kind ({} bytes)",
            envelope.ciphertext.len()
        );
        state.store.insert_refused(envelope)?;
        return Ok(Applied::default());
    }
    let applied = state.store.atomically(|store| {
        let mut applied = store.apply_permitted(verified, context)?;
        store.insert_applied(envelope, true)?;
        if applied.needs_rebuild {
            store.rebuild_views(context)?;
        }
        // The last op a removed member's log was waiting for: claims on
        // channels they owned can be judged now, in this same transaction.
        if store.frozen_last_seq(&verified.author)? == Some(verified.seq)
            && store.settle_after_removal(&verified.author, context)?
        {
            applied.needs_rebuild = true;
        }
        Ok(applied)
    });
    match applied {
        Ok(applied) => Ok(applied),
        Err(error) if error.downcast_ref::<RuleViolation>().is_some() => {
            warn!("skipping an op that breaks a rule: {error:#}");
            state.store.insert_applied(envelope, true)?;
            Ok(Applied::default())
        }
        Err(error) => Err(error),
    }
}

fn park(state: &State, envelope: &Envelope) -> Result<()> {
    ensure!(
        envelope.ciphertext.len() <= MAX_PAGE_UPDATE_BYTES,
        "op is larger than any kind may be"
    );
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
    state.store.insert_pending(envelope)
}
