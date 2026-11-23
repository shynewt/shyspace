//! Moving envelopes between the local log, the bucket, and online peers.

use anyhow::Result;
use iroh::PublicKey;
use tracing::warn;

use super::peers::peers_to_dial;
use crate::{
    ops::now_ms,
    transport::bucket::{ListedObject, SegmentName, listing_after, parse_segment_name, split_into_segments},
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
struct SegmentsRead {
    /// The first op that failed locally.
    local_failure: Option<anyhow::Error>,
    /// The (first, last) ranges of segments read whose ops all applied.
    settled: Vec<(u64, u64)>,
}

fn checked_key(author: &PublicKey) -> String {
    format!("checked_against_bucket/{}", member_id(author))
}

/// How far a log is checked against the bucket once `ranges` are: the
/// furthest point reachable from `checked` through ranges that each start
/// no later than right after it. Junk can't push it on, since only ranges
/// that matched or applied count.
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
        self.refetch_gaps()?;
        let mut local_failure = None;
        let mut has_found_new = false;
        let authors = self.state().store.everyone_ever()?;
        for author in authors {
            let held = self.state().store.last_seq(&author)?;
            if let Some(error) = self.sweep_author(&author).await? {
                local_failure.get_or_insert(error);
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
        let worth_fetching = listed
            .into_iter()
            .filter(|object| parse_segment_name(author, &object.key).is_some_and(|name| name.last > held))
            .collect();
        Ok(self.read_segments(author, worth_fetching).await?.local_failure)
    }

    /// Like `pull_author`, but lists past what we have checked against the
    /// bucket rather than past what we hold, so ops we got only over gossip
    /// are compared with the bucket's copy (#109): a segment whose range we
    /// hold is fetched only if its name's hash differs from our own copy's,
    /// and then the different version is proved forked.
    async fn sweep_author(&self, author: &PublicKey) -> Result<Option<anyhow::Error>> {
        let held = self.state().store.last_seq(author)?;
        // We can't have checked what we no longer hold (a restore leaves the
        // log out, and a fork drops ops).
        let checked = self.checked_against_bucket(author)?.min(held);
        let listed = self
            .bucket()
            .list_segments(author, Some(&listing_after(author, checked)))
            .await?;
        let mut matching = Vec::new();
        let mut worth_fetching = Vec::new();
        for object in listed {
            let Some(name) = parse_segment_name(author, &object.key) else {
                continue;
            };
            if name.last <= held && self.is_same_as_ours(author, &name)? {
                matching.push((name.first, name.last));
            } else {
                worth_fetching.push(object);
            }
        }
        let read = self.read_segments(author, worth_fetching).await?;
        matching.extend(read.settled);
        let checked_now = checked_through(checked, matching);
        if checked_now > checked {
            self.state()
                .store
                .set_meta(&checked_key(author), &checked_now.to_le_bytes())?;
        }
        Ok(read.local_failure)
    }

    /// Whether our copy of `name`'s range encodes to exactly the bytes its
    /// hash names, as an honest upload of the same ops does.
    fn is_same_as_ours(&self, author: &PublicKey, name: &SegmentName) -> Result<bool> {
        let ours = self
            .state()
            .store
            .envelopes_between(author, name.first, name.last)?;
        let count = name.last - name.first + 1;
        if u64::try_from(ours.len())? != count {
            return Ok(false);
        }
        Ok(blake3::hash(&postcard::to_stdvec(&ours)?) == name.hash)
    }

    fn checked_against_bucket(&self, author: &PublicKey) -> Result<u64> {
        let Some(saved) = self.state().store.meta(&checked_key(author))? else {
            return Ok(0);
        };
        let bytes = <[u8; 8]>::try_from(saved.as_slice())
            .map_err(|_| anyhow::anyhow!("unreadable bucket-checked mark for {author}"))?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Fetches and applies `listed` segments of `author`'s. Unsettled ones
    /// are skipped for the rest of the run.
    async fn read_segments(&self, author: &PublicKey, listed: Vec<ListedObject>) -> Result<SegmentsRead> {
        let skipped = self.skipped_segments().clone();
        let listed = listed
            .into_iter()
            .filter(|object| !skipped.contains(object))
            .collect();
        let mut segments = self.bucket().fetch_segments(author, listed);
        let mut read_all = SegmentsRead::default();
        while let Some(segment) = segments.next().await? {
            let read = self.receive_segment(author, &segment.listed.key, &segment.envelopes)?;
            if read.is_settled {
                if let Some(name) = parse_segment_name(author, &segment.listed.key) {
                    read_all.settled.push((name.first, name.last));
                }
            } else if !read.is_stopped {
                let mut skipped = self.skipped_segments();
                if skipped.len() < MAX_SKIPPED_SEGMENTS {
                    skipped.insert(segment.listed);
                }
            }
            if let Some(error) = read.local_failure {
                read_all.local_failure.get_or_insert(error);
            }
            if read.is_stopped {
                break;
            }
        }
        Ok(read_all)
    }

    /// Whether a sweep is due: the hourly net under heads and hints, which
    /// reads every author past what we hold. Timed by the wall clock, saved,
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

    /// Once per gap: if an author's next op went missing after we read past
    /// it (a parked op pushed out by the cap), fetch again what was skipped
    /// this run. Returns those authors.
    pub(super) fn refetch_gaps(&self) -> Result<Vec<PublicKey>> {
        let state = self.state();
        let mut refetched = self
            .inner
            .refetched_gaps
            .lock()
            .expect("gap list lock poisoned by an earlier panic");
        let mut refetching = Vec::new();
        for (author, next_seq) in state.store.authors_missing_next_op()? {
            if refetched.insert((author, next_seq)) {
                warn!("op {next_seq} from {author} went missing; reading their history again");
                let author_prefix = format!("ops/{}/", member_id(&author));
                self.skipped_segments()
                    .retain(|listed| !listed.key.starts_with(&author_prefix));
                refetching.push(author);
            }
        }
        Ok(refetching)
    }

    fn skipped_segments(&self) -> std::sync::MutexGuard<'_, std::collections::HashSet<ListedObject>> {
        self.inner
            .skipped_segments
            .lock()
            .expect("skipped segments lock poisoned by an earlier panic")
    }

    /// Uploads our ops that aren't in the bucket yet, in as few segments as
    /// others can still read.
    pub(crate) async fn push(&self) -> Result<()> {
        let _guard = self.inner.push_lock.lock().await;
        let me = self.me();
        let unpushed = self.state().store.unpushed(&me)?;
        if unpushed.is_empty() {
            return Ok(());
        }
        for segment in split_into_segments(&unpushed)? {
            let last = segment
                .last()
                .expect("split_into_segments never makes an empty segment")
                .seq;
            self.bucket().put_segment(segment).await?;
            self.state().store.mark_pushed(&me, last)?;
        }
        self.write_head().await
    }

    /// Lets only current members gossip with us; local, so done every pass.
    pub(super) fn refresh_gossip_members(&self) -> Result<()> {
        if let Some(net) = &self.inner.net {
            net.set_members(&self.current_members()?);
        }
        Ok(())
    }

    /// Connects to members who seem to be around (see `peers_to_dial`).
    pub(super) async fn join_peers(&self) -> Result<()> {
        let Some(net) = &self.inner.net else {
            return Ok(());
        };
        let announcements = self.bucket().peers().await?;
        let now = now_ms()?;
        let to_dial = {
            let mut last_dialed = self
                .inner
                .last_dialed_ms
                .lock()
                .expect("dial times lock poisoned by an earlier panic");
            let to_dial = peers_to_dial(&announcements, &last_dialed, now);
            for peer in &to_dial {
                last_dialed.insert(*peer, now);
            }
            to_dial
        };
        net.join(to_dial).await
    }

    pub(super) async fn announce_self(&self) {
        if let Err(error) = self.bucket().announce_peer(&self.me()).await {
            warn!("could not announce ourselves in the bucket: {error:#}");
        }
        self.nudge_peers().await;
    }

    /// Tells online peers to sync now rather than at their next interval.
    pub(crate) async fn nudge_peers(&self) {
        if let Some(net) = &self.inner.net
            && let Err(error) = net.nudge().await
        {
            warn!("could not nudge peers: {error:#}");
        }
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
