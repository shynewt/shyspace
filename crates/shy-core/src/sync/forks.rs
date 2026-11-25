//! Forked logs (#109): an author who signs two different versions of one op
//! could show each to different people. When both versions reach a device,
//! it records the fork, and neither version counts there; each sync pass
//! shares the proof through the bucket, so every device ends up the same.
//! Only the author's key can make a proof: the versions must differ in what
//! the author signed, not just in their encryption, which anyone holding the
//! epoch key could redo.

use std::collections::HashMap;

use anyhow::Result;
use bytes::Bytes;
use iroh::PublicKey;
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::receive::{AuthorKey, MAX_PENDING_EPOCHS_AHEAD, author_epoch_key};
use crate::{
    crypto,
    ops::Envelope,
    storage::CheckedFork,
    transport::bucket::{ListedProof, MAX_FORK_PROOF_BYTES},
    views::Change,
    workspace::{State, Workspace},
};

/// Bytes of new fork proof objects fetched a pass, smallest first, and at
/// least one: each object is fetched once, ever, so this only spreads out
/// the work junk listed under many names costs.
const PROOF_BYTES_PER_PASS: u64 = MAX_FORK_PROOF_BYTES;
const PROOF_FETCHES_PER_PASS: usize = 1_000;
/// Downloads of one proof object that may fail in a run before it waits
/// for the next run: a failure is never taken as an answer, as the network
/// may be to blame.
const PROOF_FETCH_TRIES: u32 = 3;

/// Two versions of one op, each one byte-for-byte as it arrived.
#[derive(Serialize, Deserialize)]
struct ForkProof {
    first: Envelope,
    second: Envelope,
}

enum Verdict {
    Forked,
    /// The same op twice, or not a proof at all.
    Refused(String),
    /// It needs this epoch's key, which hasn't arrived yet.
    NotYet(u64),
}

/// What another copy of an op we already have came to.
pub(super) enum SecondLook {
    Settled(Vec<Change>),
    /// It needs an epoch key that hasn't arrived yet; keep it until then.
    NotYet,
    /// It proves nothing, like any op that doesn't check out.
    Refused(String),
}

impl Workspace {
    /// Looks at another copy of an op whose place in its author's log is
    /// already filled.
    pub(super) fn check_for_fork(&self, state: &State, envelope: &Envelope) -> Result<SecondLook> {
        let (author, seq) = (envelope.author, envelope.seq);
        if state.store.is_forked(&author, seq)? {
            return Ok(SecondLook::Settled(Vec::new()));
        }
        let Some(stored) = state.store.applied_envelope(&author, seq)? else {
            return Ok(SecondLook::Settled(Vec::new()));
        };
        // The usual case is the same op again. One refused for its size (or
        // junk too big to keep) has only its header: nothing to compare.
        if stored == *envelope || stored.ciphertext.is_empty() {
            return Ok(SecondLook::Settled(Vec::new()));
        }
        let proof = postcard::to_stdvec(&ForkProof {
            first: stored,
            second: envelope.clone(),
        })?;
        match judge(state, &author, seq, &proof)? {
            Verdict::Forked => {
                let fork = CheckedFork { author, seq, proof };
                Ok(SecondLook::Settled(self.record_forks(state, &[fork])?))
            }
            Verdict::Refused(reason) => Ok(SecondLook::Refused(format!(
                "another copy of {author}'s op {seq}: {reason}"
            ))),
            Verdict::NotYet(_) => Ok(SecondLook::NotYet),
        }
    }

    /// Learns the forks proved in the bucket, and puts there the proofs of
    /// ours it lacks (also bringing back any that went missing). Each object
    /// is fetched once, ever: its name is new for every copy put, and what
    /// it held is remembered in the store, so junk under many names costs
    /// one download each. Smallest first, a share of bytes a pass.
    pub(super) async fn share_forks(&self) -> Result<()> {
        let bucket = self.bucket();
        let mut listed = bucket.fork_proofs().await?;
        listed.sort_by_key(|listing| listing.size);
        let mut bytes_left = PROOF_BYTES_PER_PASS;
        let mut fetches_left = PROOF_FETCHES_PER_PASS;
        let mut has_fetched = false;
        let mut proved = Vec::new();
        for listing in &listed {
            let ListedProof {
                author, seq, path, ..
            } = listing;
            let should_fetch = {
                let store = &self.state().store;
                !store.is_forked(author, *seq)? && store.should_fetch_fork_proof(path)?
            };
            if !should_fetch || self.has_failed_too_often(listing) {
                continue;
            }
            if fetches_left == 0 || (has_fetched && listing.size > bytes_left) {
                continue;
            }
            has_fetched = true;
            fetches_left -= 1;
            bytes_left = bytes_left.saturating_sub(listing.size);
            let Some(proof) = self.fetch_fork_proof(listing).await? else {
                continue;
            };
            let state = self.state();
            match judge(&state, author, *seq, &proof)? {
                Verdict::Forked => {
                    self.found_ours(listing);
                    proved.push(CheckedFork {
                        author: *author,
                        seq: *seq,
                        proof: proof.to_vec(),
                    });
                }
                Verdict::Refused(reason) => {
                    warn!("ignoring the fork proof {path}: {reason}");
                    state.store.mark_fork_proof_checked(path, None)?;
                }
                Verdict::NotYet(epoch) => state.store.mark_fork_proof_checked(path, Some(epoch))?,
            }
        }
        if !proved.is_empty() {
            let changes = self.record_forks(&self.state(), &proved)?;
            for change in changes {
                self.emit(change);
            }
        }
        self.put_back_fork_proofs(&listed).await
    }

    /// Puts each of our proofs in the bucket unless an object listed for it
    /// (one named after its hash) holds exactly it. Each such object is
    /// checked once: one found holding it is remembered for this run, one
    /// that doesn't is remembered in the store. One that can't be put
    /// doesn't stop the others; the first error is returned at the end.
    async fn put_back_fork_proofs(&self, listed: &[ListedProof]) -> Result<()> {
        let bucket = self.bucket();
        let known = self.state().store.fork_proofs()?;
        let mut first_failure = None;
        for (author, seq, proof) in known {
            let hash = blake3::hash(&proof).to_hex();
            let copies: Vec<&ListedProof> = listed
                .iter()
                .filter(|listing| listing.author == author && listing.seq == seq)
                .filter(|listing| {
                    listing
                        .path
                        .rsplit('/')
                        .next()
                        .is_some_and(|name| name.starts_with(hash.as_str()))
                })
                .collect();
            let mut is_there = copies.iter().any(|listing| self.is_ours(listing));
            for listing in copies {
                if is_there {
                    break;
                }
                if !self.state().store.should_fetch_fork_proof(&listing.path)?
                    || self.has_failed_too_often(listing)
                {
                    continue;
                }
                match self.fetch_fork_proof(listing).await? {
                    Some(stored) if *stored == proof => {
                        self.found_ours(listing);
                        is_there = true;
                    }
                    Some(_) => self.state().store.mark_fork_proof_checked(&listing.path, None)?,
                    None => {}
                }
            }
            if is_there {
                continue;
            }
            match bucket.put_fork_proof(&author, seq, proof).await {
                Ok((path, Some(version))) => {
                    self.seen_fork_proofs().insert(path, version);
                }
                // Without a version to know it by, it's checked next time.
                Ok((_, None)) => {}
                Err(error) => {
                    first_failure.get_or_insert(error);
                }
            }
        }
        first_failure.map_or(Ok(()), Err)
    }

    /// The object's bytes, or `None` if there are none to judge: gone or
    /// larger than any proof (remembered in the store, as it will never be
    /// read), or failing to download (counted for this run only).
    async fn fetch_fork_proof(&self, listing: &ListedProof) -> Result<Option<Bytes>> {
        match self.bucket().fork_proof(&listing.path).await {
            Ok(Some(proof)) => Ok(Some(proof)),
            Ok(None) => {
                self.state().store.mark_fork_proof_checked(&listing.path, None)?;
                Ok(None)
            }
            // Anyone with the bucket keys can put one a plain GET is
            // refused; it waits for the next run after a few tries.
            Err(error) => {
                warn!(
                    "passing over the fork proof {} for now: it can't be fetched: {error:#}",
                    listing.path
                );
                let mut failures = self.fork_proof_failures();
                let count = failures.entry(listing.path.clone()).or_insert(0);
                *count = count.saturating_add(1);
                Ok(None)
            }
        }
    }

    fn has_failed_too_often(&self, listing: &ListedProof) -> bool {
        self.fork_proof_failures()
            .get(&listing.path)
            .is_some_and(|count| *count >= PROOF_FETCH_TRIES)
    }

    fn fork_proof_failures(&self) -> std::sync::MutexGuard<'_, HashMap<String, u32>> {
        self.inner
            .fork_proof_failures
            .lock()
            .expect("fork proof failures lock poisoned by an earlier panic")
    }

    fn is_ours(&self, listing: &ListedProof) -> bool {
        self.seen_fork_proofs().get(&listing.path) == Some(&listing.version)
    }

    fn found_ours(&self, listing: &ListedProof) {
        self.seen_fork_proofs()
            .insert(listing.path.clone(), listing.version.clone());
    }

    fn seen_fork_proofs(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.inner
            .seen_fork_proofs
            .lock()
            .expect("fork proofs lock poisoned by an earlier panic")
    }

    fn record_forks(&self, state: &State, forks: &[CheckedFork]) -> Result<Vec<Change>> {
        for fork in forks {
            warn!(
                "{} signed two versions of their op {}; neither counts",
                fork.author, fork.seq
            );
        }
        if !state.store.record_forks(forks, self.apply_context())? {
            return Ok(Vec::new());
        }
        state.store.all_view_changes()
    }
}

/// Whether `proof` shows `author` signed two different versions of op `seq`,
/// each when they could write it.
fn judge(state: &State, author: &PublicKey, seq: u64, proof: &[u8]) -> Result<Verdict> {
    let refused = |reason: &str| Ok(Verdict::Refused(reason.to_owned()));
    if u64::try_from(proof.len())? > MAX_FORK_PROOF_BYTES {
        return refused("too large to be a proof");
    }
    let Ok(ForkProof { first, second }) = postcard::from_bytes(proof) else {
        return refused("unreadable");
    };
    let newest_epoch = state.mls.epoch()? + MAX_PENDING_EPOCHS_AHEAD;
    let mut signed = Vec::new();
    let mut missing_epoch = None;
    for version in [&first, &second] {
        if version.author != *author || version.seq != seq {
            return refused("about another op");
        }
        // Maybe we're far behind: wait for that epoch rather than refuse.
        if version.epoch > newest_epoch {
            return Ok(Verdict::NotYet(version.epoch));
        }
        match author_epoch_key(state, version)? {
            AuthorKey::Refused(reason) => return Ok(Verdict::Refused(reason)),
            AuthorKey::NotYet => missing_epoch = Some(version.epoch),
            AuthorKey::Ready(key) => match crypto::signed_body(version, &key) {
                // The header the author signed holds the epoch, so a
                // different epoch is a different version too.
                Ok(body) => signed.push((version.epoch, body)),
                Err(error) => return Ok(Verdict::Refused(format!("{error:#}"))),
            },
        }
    }
    Ok(match (missing_epoch, signed.as_slice()) {
        (Some(epoch), _) => Verdict::NotYet(epoch),
        (None, [first, second]) if first != second => Verdict::Forked,
        _ => Verdict::Refused("the same op twice".into()),
    })
}
