//! Accepting ops from others. An op is applied only if it decrypts, its
//! signature is valid, and its author was a member in the op's epoch.

use anyhow::{Context, Result, bail, ensure};
use iroh::PublicKey;
use tracing::warn;

use super::forks::SecondLook;
use crate::{
    crypto::{self, EpochKey, NewerFormat, Unreadable},
    ops::{Envelope, MAX_PAGE_UPDATE_BYTES, MAX_STORED_NUMBER, RuleViolation, VerifiedOp},
    storage::{Applied, ApplyContext},
    transport::bucket::parse_segment_name,
    views::Change,
    workspace::{State, Workspace},
};

/// Ops for epochs further ahead than this are dropped instead of parked, so
/// junk can't pile up waiting for a key that will never come.
pub(super) const MAX_PENDING_EPOCHS_AHEAD: u64 = 16;

/// An op that verified and was due, but couldn't be applied on this device
/// (a full disk, say). It stays parked and is tried again on each sync, and
/// the error shows until then. Breaking a rule is never this: such ops are
/// recorded as handled.
#[derive(Debug)]
pub(crate) struct LocalFailure;

impl std::fmt::Display for LocalFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("couldn't apply a teammate's change on this device; will try again")
    }
}

/// Like `LocalFailure`, but the op couldn't even be parked: whoever read it
/// must read it again, so it isn't lost.
#[derive(Debug)]
pub(crate) struct NotKept;

impl std::fmt::Display for NotKept {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("couldn't apply or keep a teammate's change on this device; will read it again")
    }
}

/// The last seq of `key` if it looks like a real segment of `author`'s: its
/// name matches the envelopes inside, all by `author`, in order with no
/// gaps. Envelope headers aren't signed, so this alone proves nothing; the
/// segment is settled only once its last op has applied.
fn genuine_last_seq(author: &PublicKey, key: &str, envelopes: &[Envelope]) -> Option<u64> {
    let name = parse_segment_name(author, key)?;
    let is_in_order = envelopes.iter().enumerate().all(|(index, envelope)| {
        let expected_seq = u64::try_from(index)
            .ok()
            .and_then(|index| name.first.checked_add(index));
        envelope.author == *author && Some(envelope.seq) == expected_seq
    });
    let is_whole = envelopes.first().map(|envelope| envelope.seq) == Some(name.first)
        && envelopes.last().map(|envelope| envelope.seq) == Some(name.last);
    (is_in_order && is_whole).then_some(name.last)
}

/// How many downloaded ops share one transaction: few enough that the
/// state lock is let go often, many enough to save most disk syncs.
const OPS_PER_TRANSACTION: usize = 500;

/// What applying a downloaded segment found.
#[derive(Default)]
pub(super) struct SegmentRead {
    /// The first op that failed locally.
    pub local_failure: Option<anyhow::Error>,
    /// An op couldn't even be kept: the rest of the segment waits, and the
    /// cursor stays before it.
    pub is_stopped: bool,
    /// A genuine segment whose ops all applied: nothing in it is waiting.
    pub is_settled: bool,
}

/// What retrying parked ops found.
pub(super) struct Pending {
    /// Authors held up by an op in a newer format than this version's (or
    /// junk claiming one).
    pub waiting_for_update: Vec<PublicKey>,
    /// The first parked op that failed locally again.
    pub local_failure: Option<anyhow::Error>,
}

impl Workspace {
    /// Takes an envelope from gossip or the bucket. It is stored only once it
    /// verifies; envelopes we can't apply yet are parked and retried.
    pub(crate) fn receive(&self, envelope: Envelope) -> Result<()> {
        let changes = self.receive_locked(&self.state(), &envelope)?;
        for change in changes {
            self.emit(change);
        }
        Ok(())
    }

    fn receive_locked(&self, state: &State, envelope: &Envelope) -> Result<Vec<Change>> {
        ensure!(
            (1..=MAX_STORED_NUMBER).contains(&envelope.seq) && envelope.epoch <= MAX_STORED_NUMBER,
            "op numbering out of range"
        );
        if !state.store.is_applied(&envelope.author, envelope.seq)? {
            return self.receive_new(state, envelope);
        }
        Ok(match self.check_for_fork(state, envelope)? {
            SecondLook::Settled(changes) => changes,
            SecondLook::NotYet => {
                park(state, envelope)?;
                Vec::new()
            }
            SecondLook::Refused(reason) => bail!(reason),
        })
    }

    /// Applies one downloaded segment of `author`'s ops, a batch at a time in
    /// one transaction each, so it costs a disk sync per batch rather than per
    /// op, while the state lock is let go between batches. Each op still
    /// applies on its own (a savepoint): one that breaks a rule or fails here
    /// is skipped or kept for later as when received alone. Reading resumes
    /// from what we hold, so if saving fails, the next pull reads the segment
    /// again; ops already saved are then recognised.
    pub(super) fn receive_segment(
        &self,
        author: &PublicKey,
        key: &str,
        envelopes: &[Envelope],
    ) -> Result<SegmentRead> {
        let mut read = SegmentRead::default();
        let genuine_last = genuine_last_seq(author, key, envelopes);
        if genuine_last.is_none() {
            warn!("{key} isn't a genuine segment; reading its ops anyway");
        }
        let mut batches: Vec<&[Envelope]> = envelopes.chunks(OPS_PER_TRANSACTION).collect();
        if batches.is_empty() {
            batches.push(&[]);
        }
        let last = batches.len() - 1;
        for (index, batch) in batches.into_iter().enumerate() {
            let state = self.state();
            let changes = state.store.atomically(|store| {
                let mut changes = Vec::new();
                for envelope in batch {
                    if envelope.author != *author {
                        warn!("segment {key} contains an op by someone else; skipping it");
                        continue;
                    }
                    let received = self.receive_locked(&state, envelope);
                    #[cfg(test)]
                    if self
                        .inner
                        .drop_segment_transaction
                        .swap(false, std::sync::atomic::Ordering::SeqCst)
                    {
                        store.execute_for_test("ROLLBACK");
                    }
                    ensure!(
                        store.is_in_transaction(),
                        "the database gave up the segment's transaction; it is read again"
                    );
                    match received {
                        Ok(more) => changes.extend(more),
                        Err(error) if error.downcast_ref::<NotKept>().is_some() => {
                            // We don't hold the op, so the next pull reads
                            // it again.
                            read.local_failure.get_or_insert(error);
                            read.is_stopped = true;
                            return Ok(changes);
                        }
                        Err(error) if error.downcast_ref::<LocalFailure>().is_some() => {
                            read.local_failure.get_or_insert(error);
                        }
                        Err(error) => warn!("dropping op from {key}: {error:#}"),
                    }
                }
                if index == last {
                    // Settled once its last op verified and applied (logs
                    // have no gaps, so all of it did).
                    read.is_settled = match genuine_last {
                        Some(last_seq) => store.is_applied(author, last_seq)?,
                        None => false,
                    };
                    #[cfg(test)]
                    if self
                        .inner
                        .fail_segment_save
                        .swap(false, std::sync::atomic::Ordering::SeqCst)
                    {
                        bail!("a simulated crash while saving a segment");
                    }
                }
                Ok(changes)
            })?;
            drop(state);
            for change in changes {
                self.emit(change);
            }
            if read.is_stopped {
                break;
            }
        }
        Ok(read)
    }

    /// An op for a place in its author's log we haven't filled yet.
    fn receive_new(&self, state: &State, envelope: &Envelope) -> Result<Vec<Change>> {
        let verified = match verify_or_wait(state, envelope) {
            Err(error) if error.downcast_ref::<Unreadable>().is_some() => {
                return skip_unreadable(state, envelope);
            }
            verified => verified?,
        };
        Ok(match verified {
            Some(verified) if is_ready(state, &verified)? => {
                match self.apply_with_alert(state, envelope, &verified) {
                    Ok(changes) => changes,
                    Err(error) => {
                        if let Err(not_kept) = self.keep_for_retry(state, envelope) {
                            warn!("couldn't apply a teammate's change: {error:#}");
                            return Err(not_kept.context(NotKept));
                        }
                        return Err(error.context(LocalFailure));
                    }
                }
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

    fn keep_for_retry(&self, state: &State, envelope: &Envelope) -> Result<()> {
        #[cfg(test)]
        if self
            .inner
            .fail_next_park
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            bail!("a simulated full disk");
        }
        park(state, envelope)
    }

    fn apply_with_alert(
        &self,
        state: &State,
        envelope: &Envelope,
        verified: &VerifiedOp,
    ) -> Result<Vec<Change>> {
        #[cfg(test)]
        if self
            .inner
            .fail_next_apply
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            bail!("a simulated full disk");
        }
        let applied = apply_and_record(state, envelope, verified, self.apply_context())?;
        // The op is saved now; what's left only tells the app about it, so a
        // failure here must not count as failing to apply.
        let mut changes = applied.changes;
        for readable in &applied.readable {
            match self.alert_for(state, readable) {
                Ok(alert) => changes.extend(alert),
                Err(error) => warn!("couldn't decide whether to alert: {error:#}"),
            }
            match self.page_mention_for(state, readable) {
                Ok(more) => changes.extend(more),
                Err(error) => warn!("couldn't look for a page mention: {error:#}"),
            }
        }
        if applied.needs_rebuild {
            match state.store.all_view_changes() {
                Ok(more) => changes.extend(more),
                Err(error) => warn!("couldn't list what a rebuild changed: {error:#}"),
            }
            match self.forget_undone_page_mentions(state) {
                Ok(more) => changes.extend(more),
                Err(error) => warn!("couldn't forget undone page mentions: {error:#}"),
            }
        }
        Ok(changes)
    }

    /// Retries parked ops, for example after a new epoch key arrived. One
    /// that fails locally stays parked, and the others still get their turn.
    pub(super) fn apply_pending(&self) -> Result<Pending> {
        let mut waiting_for_update = Vec::new();
        let mut local_failure = None;
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
                        match self.apply_with_alert(&state, &envelope, &verified) {
                            Ok(more) => changes.extend(more),
                            Err(error) => {
                                local_failure.get_or_insert(error.context(LocalFailure));
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) if error.downcast_ref::<Unreadable>().is_some() => {
                        skip_unreadable(&state, &envelope)?;
                        state.store.delete_pending(&envelope)?;
                    }
                    Err(error) if error.downcast_ref::<NewerFormat>().is_some() => {
                        // Only the op its author needs next blocks anything;
                        // a far-ahead one can't raise a false alarm.
                        if envelope.seq == state.store.next_seq(&envelope.author)?
                            && !waiting_for_update.contains(&envelope.author)
                        {
                            waiting_for_update.push(envelope.author);
                        }
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
        Ok(Pending {
            waiting_for_update,
            local_failure,
        })
    }
}

/// Signed junk its author can't take back: recorded as handled in its turn,
/// so their later ops aren't held up. It's kept whole when it fits the
/// largest op any kind may be (it never applies), so a real op for the same
/// seq is still proved a fork. Before its turn it isn't kept, and gap repair reads it
/// again once later ops wait behind it.
fn skip_unreadable(state: &State, envelope: &Envelope) -> Result<Vec<Change>> {
    let is_next = envelope.seq == 1 || state.store.is_applied(&envelope.author, envelope.seq - 1)?;
    if is_next {
        warn!("skipping a signed op that no version can read");
        if envelope.ciphertext.len() <= MAX_PAGE_UPDATE_BYTES {
            state.store.insert_applied(envelope, true)?;
        } else {
            state.store.insert_refused(envelope)?;
        }
    }
    Ok(Vec::new())
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

/// Keeps `envelope` to try again later. Refusing it for what it is (too
/// large, from a non-member, too far ahead) is a plain error; failing to read
/// or write our own data is `NotKept`, so whoever read it reads it again.
fn park(state: &State, envelope: &Envelope) -> Result<()> {
    ensure!(
        envelope.ciphertext.len() <= MAX_PAGE_UPDATE_BYTES,
        "op is larger than any kind may be"
    );
    let was_member = state.store.was_ever_member(&envelope.author).context(NotKept)?;
    ensure!(was_member, "op from a non-member");
    // A device that only reads has no group: its newest epoch is the
    // newest it was handed.
    let current_epoch = match state.mls.is_active() {
        true => state.mls.epoch(),
        false => state.store.newest_epoch(),
    }
    .context(NotKept)?;
    ensure!(
        envelope.epoch <= current_epoch + MAX_PENDING_EPOCHS_AHEAD,
        "op is for an epoch too far ahead ({})",
        envelope.epoch
    );
    state.store.insert_pending(envelope).context(NotKept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(author: PublicKey, seq: u64) -> Envelope {
        Envelope {
            author,
            seq,
            epoch: 1,
            nonce: [0; 24],
            ciphertext: Vec::new(),
        }
    }

    #[test]
    fn a_segment_looks_genuine_only_if_its_name_matches_every_envelope() {
        let alice = iroh::SecretKey::generate().public();
        let bob = iroh::SecretKey::generate().public();
        let key = crate::transport::bucket::segment_name_for(&alice, 3, 5, &[1; 16], b"segment");
        let run = |author: PublicKey, seqs: &[u64]| -> Vec<Envelope> {
            seqs.iter().map(|seq| envelope(author, *seq)).collect()
        };

        assert_eq!(genuine_last_seq(&alice, &key, &run(alice, &[3, 4, 5])), Some(5));
        assert_eq!(
            genuine_last_seq(&alice, &key, &run(alice, &[3, 4])),
            None,
            "ends early"
        );
        assert_eq!(
            genuine_last_seq(&alice, &key, &run(alice, &[3, 5])),
            None,
            "a gap"
        );
        assert_eq!(
            genuine_last_seq(&alice, &key, &run(alice, &[4, 5])),
            None,
            "starts late"
        );
        assert_eq!(genuine_last_seq(&alice, &key, &[]), None, "empty");
        let mut mixed = run(alice, &[3, 4, 5]);
        mixed[1].author = bob;
        assert_eq!(genuine_last_seq(&alice, &key, &mixed), None, "someone else's op");
    }
}
