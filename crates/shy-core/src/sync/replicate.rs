//! Moving envelopes between the local log and the bucket.

use anyhow::Result;
use iroh::PublicKey;
use tracing::warn;

use super::receive::SegmentRead;
use crate::{
    crypto,
    ops::{Envelope, now_ms},
    transport::bucket::{
        FetchedSegment, ListedObject, MAX_OBJECT_SIZE, SegmentName, listing_after, parse_segment_name,
        segment_hash, split_into_segments,
    },
    views::member_id,
    workspace::Workspace,
};

/// At most this many skipped objects are remembered; past it, the rest are
/// fetched again each pass rather than using unbounded memory.
const MAX_SKIPPED_SEGMENTS: usize = 10_000;
/// How often a sweep reads every author past what we hold, at most.
const SWEEP_EVERY_MS: i64 = 60 * 60 * 1000;
const LAST_SWEEP_META: &str = "last_sweep_ms";

#[derive(Default)]
pub(super) struct SegmentsRead {
    /// The first op that failed locally.
    pub local_failure: Option<anyhow::Error>,
    /// The (first, last) ranges of segments read that hold exactly our log.
    pub matching: Vec<(u64, u64)>,
    /// An op couldn't even be kept, so reading stopped for this author.
    pub is_stopped: bool,
}

impl SegmentsRead {
    pub(super) fn absorb(&mut self, other: SegmentsRead) {
        self.matching.extend(other.matching);
        if let Some(error) = other.local_failure {
            self.local_failure.get_or_insert(error);
        }
        self.is_stopped |= other.is_stopped;
    }
}

fn checked_key(author: &PublicKey) -> String {
    format!("checked_against_bucket/{}", member_id(author))
}

/// How far a log is checked against the bucket once `ranges` are: the
/// furthest point reachable from `checked` through ranges that each start
/// no later than right after it. Only ranges whose envelopes are exactly
/// ours count, so junk can't push it on.
fn checked_through(checked: u64, mut ranges: Vec<(u64, u64)>) -> u64 {
    ranges.sort_unstable();
    ranges.into_iter().fold(checked, |reached, (first, last)| {
        if first <= reached.saturating_add(1) {
            reached.max(last)
        } else {
            reached
        }
    })
}

impl Workspace {
    /// Reads new segments from everyone who was ever a member: a newcomer
    /// also needs the history of people removed before they joined. Our own
    /// log is included so a restored device learns what it already wrote.
    /// Each author is listed from what we hold of their log, never from a
    /// stored bucket key, so objects others put in the bucket can't make us
    /// skip any of it. An op that fails locally is parked and doesn't hold
    /// reading back, unless it couldn't even be parked; the first such error
    /// is returned at the end. Returns whether anything new came.
    pub(crate) async fn pull(&self) -> Result<bool> {
        let gaps = self.gaps_to_repair()?;
        let mut local_failure = None;
        let mut has_found_new = false;
        let authors = self.state().store.everyone_ever()?;
        for author in authors {
            let held = self.state().store.last_seq(&author)?;
            match self.sweep_author(&author).await? {
                Some(error) => {
                    local_failure.get_or_insert(error);
                }
                None => {
                    for gap in gaps.iter().filter(|(gap_author, _)| *gap_author == author) {
                        self.mark_gap_repaired(*gap);
                    }
                }
            }
            has_found_new |= self.state().store.last_seq(&author)? > held;
        }
        // A sweep cut short by our own failure isn't done; try again next pass.
        if local_failure.is_none() {
            let now = now_ms()?;
            self.state().store.set_meta(LAST_SWEEP_META, &now.to_le_bytes())?;
        }
        local_failure.map_or(Ok(has_found_new), Err)
    }

    /// Reads `author`'s segments that may hold ops past what we hold (names
    /// sort by their last seq, so listing past what we hold finds them all);
    /// returns the first op that failed locally. Only names that parse are
    /// fetched, so junk costs a line in a listing, never a download.
    pub(crate) async fn pull_author(&self, author: &PublicKey) -> Result<Option<anyhow::Error>> {
        let held = self.state().store.last_seq(author)?;
        let listed = self
            .bucket()
            .list_segments(author, Some(&listing_after(author, held)))
            .await?;
        let candidates = self.named_unskipped(author, listed);
        let mut chain = self.read_covering_chain(author, &candidates).await?;
        if chain.read.is_stopped {
            return Ok(chain.read.local_failure);
        }
        let held = self.state().store.last_seq(author)?;
        let rest = candidates
            .into_iter()
            .filter(|(object, name)| !chain.tried.contains(&object.key) && name.last > held)
            .collect();
        let (rest_read, _) = self.read_rest(author, rest).await?;
        chain.read.absorb(rest_read);
        Ok(chain.read.local_failure)
    }

    /// The listed objects whose names are `author`'s segments, with those
    /// names, leaving out those already found unsettled this run.
    fn named_unskipped(
        &self,
        author: &PublicKey,
        listed: Vec<ListedObject>,
    ) -> Vec<(ListedObject, SegmentName)> {
        let skipped = self.skipped_segments().clone();
        listed
            .into_iter()
            .filter(|object| !skipped.contains(object))
            .filter_map(|object| parse_segment_name(author, &object.key).map(|name| (object, name)))
            .collect()
    }

    /// Like `pull_author`, but lists past what we have checked against the
    /// bucket rather than past what we hold, so ops we got only over gossip
    /// are compared with the bucket's copy (#109): a segment whose range we
    /// hold is fetched only if its name's hash differs from our own copy's,
    /// and then the different version is proved forked.
    async fn sweep_author(&self, author: &PublicKey) -> Result<Option<anyhow::Error>> {
        let held = self.state().store.last_seq(author)?;
        // We can't have checked what we no longer hold (a restore leaves the
        // log out).
        let stored_checked = self.checked_against_bucket(author)?;
        let checked = stored_checked.min(held);
        if checked < stored_checked {
            self.save_checked(author, checked)?;
        }
        let listed = self
            .bucket()
            .list_segments(author, Some(&listing_after(author, checked)))
            .await?;
        let candidates = self.named_unskipped(author, listed);
        let chain = self.read_covering_chain(author, &candidates).await?;
        let mut read = chain.read;
        if !read.is_stopped {
            let held = self.state().store.last_seq(author)?;
            let mut differing = Vec::new();
            let mut past_held = Vec::new();
            for (object, name) in candidates {
                if chain.tried.contains(&object.key) {
                    continue;
                }
                if name.last > held {
                    past_held.push((object, name));
                } else if self.is_same_as_ours(author, &name)? {
                    read.matching.push((name.first, name.last));
                } else {
                    differing.push(object);
                }
            }
            read.absorb(self.read_segments(author, differing).await?);
            if !read.is_stopped {
                let (rest_read, unread) = self.read_rest(author, past_held).await?;
                read.absorb(rest_read);
                // A covered pack left unread may still hold a second version
                // of an op; compare it like any other object we hold.
                let mut differing = Vec::new();
                for (object, name) in unread {
                    if self.is_same_as_ours(author, &name)? {
                        read.matching.push((name.first, name.last));
                    } else {
                        differing.push(object);
                    }
                }
                read.absorb(self.read_segments(author, differing).await?);
            }
        }
        let checked_now = checked_through(checked, read.matching);
        if checked_now > checked {
            self.save_checked(author, checked_now)?;
        }
        Ok(read.local_failure)
    }

    /// Whether our copy of `name`'s range encodes to exactly the bytes its
    /// hash names, as an honest upload of the same ops does. A range bigger
    /// than any segment can't be, so it isn't loaded.
    pub(super) fn is_same_as_ours(&self, author: &PublicKey, name: &SegmentName) -> Result<bool> {
        let size = self
            .state()
            .store
            .envelope_bytes_between(author, name.first, name.last)?;
        if size > MAX_OBJECT_SIZE {
            return Ok(false);
        }
        let ours = self
            .state()
            .store
            .envelopes_between(author, name.first, name.last)?;
        if u64::try_from(ours.len())? != name.last - name.first + 1 {
            return Ok(false);
        }
        Ok(segment_hash(&name.nonce, &postcard::to_stdvec(&ours)?) == name.hash)
    }

    /// Whether every one of these envelopes is the op in our log: the same
    /// bytes, or the same body its author signed (anyone with the epoch key
    /// can encrypt an op again), or a seq that is a proved fork. Only then
    /// does a segment count as checked against the bucket: headers aren't
    /// signed, so junk with real headers must not count. A genuine segment's
    /// seqs run without gaps, so the two are compared pairwise.
    fn is_all_in_our_log(&self, author: &PublicKey, envelopes: &[Envelope]) -> Result<bool> {
        let (Some(first), Some(last)) = (envelopes.first(), envelopes.last()) else {
            return Ok(false);
        };
        let ours = self
            .state()
            .store
            .envelopes_between(author, first.seq, last.seq)?;
        if ours.len() != envelopes.len() {
            return Ok(false);
        }
        for (theirs, stored) in envelopes.iter().zip(&ours) {
            // One refused for its size (or junk too big to keep) has only its
            // header: nothing to compare, as when checking for forks.
            let is_ours = stored.seq == theirs.seq
                && (stored == theirs
                    || stored.ciphertext.is_empty()
                    || self.state().store.is_forked(author, theirs.seq)?
                    || self.is_same_signed(stored, theirs)?);
            if !is_ours {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn is_same_signed(&self, stored: &Envelope, theirs: &Envelope) -> Result<bool> {
        let state = self.state();
        let (Some(stored_key), Some(their_key)) = (
            state.store.epoch_key(stored.epoch)?,
            state.store.epoch_key(theirs.epoch)?,
        ) else {
            return Ok(false);
        };
        match (
            crypto::signed_body(stored, &stored_key),
            crypto::signed_body(theirs, &their_key),
        ) {
            (Ok(ours), Ok(their)) => Ok(ours == their),
            _ => Ok(false),
        }
    }

    fn save_checked(&self, author: &PublicKey, checked: u64) -> Result<()> {
        self.state()
            .store
            .set_meta(&checked_key(author), &checked.to_le_bytes())
    }

    pub(crate) fn checked_against_bucket(&self, author: &PublicKey) -> Result<u64> {
        let Some(saved) = self.state().store.meta(&checked_key(author))? else {
            return Ok(0);
        };
        let bytes = <[u8; 8]>::try_from(saved.as_slice())
            .map_err(|_| anyhow::anyhow!("unreadable bucket-checked mark for {author}"))?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Fetches and applies `listed` segments of `author`'s. Unsettled ones
    /// are skipped for the rest of the run.
    pub(super) async fn read_segments(
        &self,
        author: &PublicKey,
        listed: Vec<ListedObject>,
    ) -> Result<SegmentsRead> {
        let mut segments = self.bucket().fetch_segments(author, listed);
        let mut read_all = SegmentsRead::default();
        while let Some(segment) = segments.next().await? {
            if self.take_segment(author, segment, &mut read_all)?.is_stopped {
                read_all.is_stopped = true;
                break;
            }
        }
        Ok(read_all)
    }

    /// Applies one fetched segment of `author`'s, noting in `read_all` what
    /// it found; one that doesn't settle is skipped for the rest of the run.
    pub(super) fn take_segment(
        &self,
        author: &PublicKey,
        segment: FetchedSegment,
        read_all: &mut SegmentsRead,
    ) -> Result<SegmentRead> {
        let mut read = self.receive_segment(author, &segment.listed.key, &segment.envelopes)?;
        let name = parse_segment_name(author, &segment.listed.key);
        if let Some(name) = &name
            && read.is_settled
        {
            self.note_found_in_bucket(author, name.first, name.last)?;
        }
        let is_checked = read.is_settled && self.is_all_in_our_log(author, &segment.envelopes)?;
        match name {
            Some(name) if is_checked => read_all.matching.push((name.first, name.last)),
            _ if !read.is_stopped => {
                let mut skipped = self.skipped_segments();
                if skipped.len() < MAX_SKIPPED_SEGMENTS {
                    skipped.insert(segment.listed);
                }
            }
            _ => {}
        }
        if let Some(error) = read.local_failure.take() {
            read_all.local_failure.get_or_insert(error);
        }
        Ok(read)
    }

    /// Whether a sweep is due: the hourly net under heads and hints, which
    /// reads every author past what it has checked against the bucket. Timed by the wall clock, saved,
    /// so a restart or sleep doesn't put it off; a clock set back makes it
    /// due at once.
    pub(crate) fn is_sweep_due(&self) -> Result<bool> {
        let Some(saved) = self.state().store.meta(LAST_SWEEP_META)? else {
            return Ok(true);
        };
        let Ok(bytes) = <[u8; 8]>::try_from(saved.as_slice()) else {
            warn!("the time of the last sweep is unreadable; sweeping now");
            return Ok(true);
        };
        let last_ms = i64::from_le_bytes(bytes);
        let now = now_ms()?;
        Ok(now < last_ms || now.saturating_sub(last_ms) >= SWEEP_EVERY_MS)
    }

    /// As if the last sweep had been at `last_ms`.
    #[cfg(test)]
    pub(crate) fn set_last_sweep(&self, last_ms: i64) {
        self.state()
            .store
            .set_meta(LAST_SWEEP_META, &last_ms.to_le_bytes())
            .expect("the store is writable in tests");
    }
    /// Uploads our ops that aren't in the bucket yet, in as few segments as
    /// others can still read, then any packs due (see `packing`).
    pub(crate) async fn push(&self) -> Result<()> {
        let _guard = self.inner.push_lock.lock().await;
        let me = self.me();
        // Before uploading, so a store that never packed starts from here.
        let packed_through = self.packed_through()?;
        let unpushed = self.state().store.unpushed(&me)?;
        if !unpushed.is_empty() {
            for segment in split_into_segments(&unpushed)? {
                let last = segment
                    .last()
                    .expect("split_into_segments never makes an empty segment")
                    .seq;
                self.bucket().put_segment(segment).await?;
                self.state().store.mark_pushed(&me, last)?;
            }
            self.write_head().await?;
        }
        self.upload_due_packs(packed_through).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checked_mark_moves_only_through_ranges_that_join_up() {
        assert_eq!(checked_through(5, vec![(6, 9), (10, 12)]), 12);
        assert_eq!(
            checked_through(5, vec![(10, 12), (6, 9)]),
            12,
            "order doesn't matter"
        );
        assert_eq!(checked_through(5, vec![(3, 7)]), 7, "an overlap carries on");
        assert_eq!(checked_through(5, vec![(8, 20)]), 5, "a gap stops it");
        assert_eq!(checked_through(5, vec![(1, 4)]), 5, "it never moves back");
        assert_eq!(checked_through(0, vec![(1, u64::MAX)]), u64::MAX);
    }
}
