//! Accepting ops from others. An op is applied only if it decrypts, its
//! signature is valid, and its author was a member in the op's epoch.

use anyhow::{Result, ensure};
use tracing::warn;

use crate::{
    crypto,
    ops::{Envelope, VerifiedOp},
    workspace::{State, Workspace},
};

/// Ops for epochs further ahead than this are dropped instead of parked, so
/// junk can't pile up waiting for a key that will never come.
const MAX_PENDING_EPOCHS_AHEAD: u64 = 16;
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
        let change = {
            let state = self.state();
            if state.store.is_applied(&envelope.author, envelope.seq)? {
                return Ok(());
            }
            match verify(&state, &envelope)? {
                Some(verified) if is_ready(&state, &verified)? => {
                    apply_and_record(&state, &envelope, &verified)?
                }
                _ => {
                    park(&state, &envelope)?;
                    None
                }
            }
        };
        if let Some(change) = change {
            self.emit(change);
        }
        Ok(())
    }

    /// Retries parked ops, for example after a new epoch key arrived.
    pub(super) fn apply_pending(&self) -> Result<()> {
        let changes = {
            let state = self.state();
            let mut changes = Vec::new();
            for envelope in state.store.pending()? {
                if state.store.is_applied(&envelope.author, envelope.seq)? {
                    state.store.delete_pending(&envelope)?;
                    continue;
                }
                match verify(&state, &envelope) {
                    Ok(Some(verified)) if is_ready(&state, &verified)? => {
                        changes.extend(apply_and_record(&state, &envelope, &verified)?);
                    }
                    Ok(_) => {}
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
        Ok(())
    }
}

/// `Ok(None)` when we lack the epoch key yet.
fn verify(state: &State, envelope: &Envelope) -> Result<Option<VerifiedOp>> {
    let Some(key) = state.store.epoch_key(envelope.epoch)? else {
        return Ok(None);
    };
    ensure!(
        state.store.was_member_at(envelope.epoch, &envelope.author)?,
        "author was not a member in epoch {}",
        envelope.epoch
    );
    ensure!(
        !state.store.is_beyond_frozen_log(&envelope.author, envelope.seq)?,
        "op written after its author was removed"
    );
    Ok(Some(crypto::open(envelope, &key)?))
}

/// An op applies only right after its author's previous one, so every
/// author's log has no gaps. That makes a removed member's frozen log exact:
/// nothing can be slipped in below the freeze. It also waits while its clock
/// is implausibly far ahead of ours.
fn is_ready(state: &State, verified: &VerifiedOp) -> Result<bool> {
    let follows_previous = verified.seq == 1 || state.store.is_applied(&verified.author, verified.seq - 1)?;
    Ok(follows_previous && state.store.accepts_clock(verified.body.lamport)?)
}

fn apply_and_record(
    state: &State,
    envelope: &Envelope,
    verified: &VerifiedOp,
) -> Result<Option<crate::views::Change>> {
    state.store.atomically(|store| {
        let change = store.apply(verified)?;
        store.insert_applied(envelope, true)?;
        Ok(change)
    })
}

fn park(state: &State, envelope: &Envelope) -> Result<()> {
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
