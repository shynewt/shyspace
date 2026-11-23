//! Forked logs (#109): an author who signs two different versions of one op
//! could show each to different people. When both versions reach a device,
//! it records the fork, and neither version counts there; each sync pass
//! shares the proof through the bucket, so every device ends up the same.
//! Only the author's key can make a proof: the versions must differ in what
//! the author signed, not just in their encryption, which anyone holding the
//! epoch key could redo.

use std::collections::HashMap;

use anyhow::Result;
use iroh::PublicKey;
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::receive::{AuthorKey, MAX_PENDING_EPOCHS_AHEAD, author_epoch_key};
use crate::{
    crypto,
    ops::Envelope,
    storage::CheckedFork,
    transport::bucket::{ListedProof, MAX_FORK_PROOF_BYTES, fork_proof_path},
    views::Change,
    workspace::{State, Workspace},
};

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
    /// ours it lacks (also bringing back any that went missing).
    pub(super) async fn share_forks(&self) -> Result<()> {
        let bucket = self.bucket();
        let listed = bucket.fork_proofs().await?;
        let mut proved = Vec::new();
        for listing in &listed {
            let ListedProof {
                author, seq, path, ..
            } = listing;
            let should_fetch = {
                let store = &self.state().store;
                !self.is_seen(listing)
                    && !store.is_forked(author, *seq)?
                    && store.should_fetch_fork_proof(path)?
            };
            if !should_fetch {
                continue;
            }
            let Some(proof) = bucket.fork_proof(path).await? else {
                continue;
            };
            // Someone overwrote it: not the proof its name promises. Its
            // holders put it back, which makes a new version to look at.
            if !is_named_for(path, &proof) {
                warn!("the fork proof {path} was overwritten");
                self.mark_seen(listing);
                continue;
            }
            let state = self.state();
            match judge(&state, author, *seq, &proof)? {
                Verdict::Forked => proved.push(CheckedFork {
                    author: *author,
                    seq: *seq,
                    proof: proof.to_vec(),
                }),
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

    /// Puts our proofs in the bucket where they're missing or were
    /// overwritten. Each version of an object is fetched at most once.
    async fn put_back_fork_proofs(&self, listed: &[ListedProof]) -> Result<()> {
        let bucket = self.bucket();
        let known = self.state().store.fork_proofs()?;
        for (author, seq, proof) in known {
            let path = fork_proof_path(&author, seq, &proof);
            let listing = listed.iter().find(|listing| listing.path == path);
            let is_intact = match listing {
                None => false,
                Some(listing) if self.is_seen(listing) => true,
                Some(listing) => {
                    let is_ours = bucket
                        .fork_proof(&path)
                        .await?
                        .is_some_and(|stored| *stored == proof);
                    if is_ours {
                        self.mark_seen(listing);
                    }
                    is_ours
                }
            };
            if !is_intact {
                bucket.put_fork_proof(&author, seq, proof).await?;
            }
        }
        Ok(())
    }

    fn is_seen(&self, listing: &ListedProof) -> bool {
        self.seen_fork_proofs().get(&listing.path) == Some(&listing.version)
    }

    fn mark_seen(&self, listing: &ListedProof) {
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

/// Whether `proof` is what the bucket path it came from was named after.
fn is_named_for(path: &str, proof: &[u8]) -> bool {
    path.rsplit('/').next() == Some(blake3::hash(proof).to_hex().as_str())
}
