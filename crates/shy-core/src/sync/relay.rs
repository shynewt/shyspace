//! Relaying: a member uploads someone else's ops that reached it live but
//! never reached the bucket, because the author's device died (or lost the
//! bucket) right after sending. Otherwise newcomers would never get them,
//! and a restored admin would reuse their seqs, which every device would
//! then prove a fork. The ops are the author's own signed envelopes, byte
//! for byte, so a relay can't forge or change anything.

use std::collections::HashMap;

use anyhow::Result;
use iroh::PublicKey;

use crate::{
    ops::now_ms,
    transport::bucket::{listing_after, parse_segment_name, split_into_segments},
    views::PresenceState,
    workspace::Workspace,
};

/// How long ops may be missing from the bucket before the first relayer
/// looks: uploads normally follow within a second.
pub(crate) const RELAY_AFTER_MS: i64 = 2 * 60 * 1000;
/// How much longer each next member in line waits, so usually one relays.
pub(crate) const RELAY_STEP_MS: i64 = 60 * 1000;

/// What we know each author has in the bucket, and since when we have held
/// more; in memory only.
#[derive(Debug, Default)]
pub struct RelayState {
    /// Seqs the author signed a hint for, or that we found there ourselves.
    uploaded_through: HashMap<PublicKey, u64>,
    /// When we first held more of the author's log than that.
    waiting_since_ms: HashMap<PublicKey, i64>,
}

/// The runs of seqs after `known` up to `held` that no range in `covered`
/// holds.
fn missing_runs(known: u64, held: u64, mut covered: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    covered.sort_unstable();
    let mut missing = Vec::new();
    let mut next = known + 1;
    for (first, last) in covered {
        if next > held {
            break;
        }
        if first > next {
            missing.push((next, (first - 1).min(held)));
        }
        next = next.max(last.saturating_add(1));
    }
    if next <= held {
        missing.push((next, held));
    }
    missing
}

/// Orders members by how close their key is to `author`'s, so each author
/// has a fixed first relayer.
fn distance(member: &PublicKey, author: &PublicKey) -> [u8; 32] {
    let mut distance = [0; 32];
    for (index, byte) in distance.iter_mut().enumerate() {
        *byte = member.as_bytes()[index] ^ author.as_bytes()[index];
    }
    distance
}

impl Workspace {
    /// Notes that `author` says the bucket has their log through `seq`.
    pub(super) fn note_uploaded(&self, author: &PublicKey, seq: u64) {
        let mut relays = self.relays();
        let known = relays.uploaded_through.entry(*author).or_default();
        *known = (*known).max(seq);
    }

    /// Relays, for each author, the ops we applied that the bucket has
    /// lacked for a while. Our turn comes after the members online whose
    /// keys are closer to the author's, and we look in the bucket first, so
    /// usually one member uploads and the rest find it there.
    pub(super) async fn relay_live_only_ops(&self) -> Result<()> {
        let me = self.me();
        let now = now_ms()?;
        let members = self.current_members()?;
        if !members.contains(&me) {
            return Ok(());
        }
        let authors = self.state().store.everyone_ever()?;
        for author in authors {
            if author == me {
                continue;
            }
            let held = self.state().store.last_seq(&author)?;
            let known = self.known_uploaded(&author)?;
            if held <= known {
                self.relays().waiting_since_ms.remove(&author);
                continue;
            }
            let waiting_since = *self.relays().waiting_since_ms.entry(author).or_insert(now);
            let rank = self.relay_rank(&author, &members, now);
            let wait = RELAY_AFTER_MS + i64::try_from(rank)?.saturating_mul(RELAY_STEP_MS);
            if now.saturating_sub(waiting_since) < wait {
                continue;
            }
            self.relay(&author, known, held).await?;
            self.note_uploaded(&author, held);
            self.relays().waiting_since_ms.remove(&author);
        }
        Ok(())
    }

    fn known_uploaded(&self, author: &PublicKey) -> Result<u64> {
        let noted = self.relays().uploaded_through.get(author).copied().unwrap_or(0);
        Ok(noted.max(self.checked_against_bucket(author)?))
    }

    /// Our place in line to relay `author`'s ops: among current members
    /// other than the author who seem online (and us), by closeness to the
    /// author's key.
    fn relay_rank(&self, author: &PublicKey, members: &[PublicKey], now: i64) -> usize {
        let me = self.me();
        let mut in_line: Vec<&PublicKey> = members
            .iter()
            .filter(|member| {
                *member != author
                    && (**member == me || self.presence_of(member, now) != PresenceState::Unreachable)
            })
            .collect();
        in_line.sort_by_key(|member| distance(member, author));
        in_line
            .iter()
            .position(|member| **member == me)
            .expect("we are a current member, so in line")
    }

    /// Uploads `author`'s ops after `known` up to `held` that no listed
    /// segment holds exactly as we do (a name's hash is checked against our
    /// copy, so junk names can't stand in for them). Ops kept without their
    /// content (refused for size) aren't relayed.
    async fn relay(&self, author: &PublicKey, known: u64, held: u64) -> Result<()> {
        let listed = self
            .bucket()
            .list_segments(author, Some(&listing_after(author, known)))
            .await?;
        let mut covered = Vec::new();
        for object in listed {
            if let Some(name) = parse_segment_name(author, &object.key)
                && name.last <= held
                && self.is_same_as_ours(author, &name)?
            {
                covered.push((name.first, name.last));
            }
        }
        for (first, last) in missing_runs(known, held, covered) {
            let envelopes = self.state().store.envelopes_between(author, first, last)?;
            for run in envelopes.split(|envelope| envelope.ciphertext.is_empty()) {
                for segment in split_into_segments(run)? {
                    self.bucket().put_segment(segment).await?;
                }
            }
        }
        Ok(())
    }

    fn relays(&self) -> std::sync::MutexGuard<'_, RelayState> {
        self.inner
            .relays
            .lock()
            .expect("relay lock poisoned by an earlier panic")
    }

    /// As if every wait for a relay had started `by_ms` earlier.
    #[cfg(test)]
    pub(crate) fn backdate_relay_waits(&self, by_ms: i64) {
        for since in self.relays().waiting_since_ms.values_mut() {
            *since -= by_ms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_runs_are_what_no_covered_range_holds() {
        assert_eq!(missing_runs(3, 10, Vec::new()), [(4, 10)]);
        assert_eq!(missing_runs(3, 10, vec![(6, 7), (1, 4)]), [(5, 5), (8, 10)]);
        assert_eq!(missing_runs(3, 10, vec![(4, 12)]), []);
        assert_eq!(missing_runs(3, 10, vec![(4, 6), (7, 9)]), [(10, 10)]);
        assert_eq!(missing_runs(3, 10, vec![(12, 20)]), [(4, 10)]);
    }
}
