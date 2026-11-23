//! Moving envelopes between the local log, the bucket, and online peers.

use std::time::Duration;

use anyhow::Result;
use iroh::PublicKey;
use tracing::warn;

use super::peers::peers_to_dial;
use crate::{
    ops::now_ms,
    transport::bucket::{ListedObject, listing_after, parse_segment_name, split_into_segments},
    views::member_id,
    workspace::Workspace,
};

/// At most this many skipped objects are remembered; past it, the rest are
/// fetched again each pass rather than using unbounded memory.
const MAX_SKIPPED_SEGMENTS: usize = 10_000;
/// How often every author is listed whole, at most.
const WHOLE_LISTING_EVERY_MS: i64 = 60 * 60 * 1000;
const LAST_WHOLE_LISTING_META: &str = "last_whole_listing_ms";

/// How much of an author's segments to list.
pub(crate) enum Listing {
    /// Only names that sort past what we hold.
    PastHeld,
    /// All of them, for older names we may hold part of.
    Whole,
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
        let refetching = self.refetch_gaps()?;
        let is_whole_listing_due = self.is_whole_listing_due()?;
        let mut local_failure = None;
        let mut has_found_new = false;
        let authors = self.state().store.everyone_ever()?;
        for author in authors {
            let held = self.state().store.last_seq(&author)?;
            let is_gap_listed_whole = refetching
                .iter()
                .any(|(gap_author, listing)| *gap_author == author && matches!(listing, Listing::Whole));
            let listing = match is_whole_listing_due || is_gap_listed_whole {
                true => Listing::Whole,
                false => Listing::PastHeld,
            };
            if let Some(error) = self.pull_author(&author, listing).await? {
                local_failure.get_or_insert(error);
            }
            has_found_new |= self.state().store.last_seq(&author)? > held;
        }
        // A listing cut short by our own failure isn't done; try again next pass.
        if is_whole_listing_due && local_failure.is_none() {
            let now = now_ms()?;
            self.state()
                .store
                .set_meta(LAST_WHOLE_LISTING_META, &now.to_le_bytes())?;
        }
        local_failure.map_or(Ok(has_found_new), Err)
    }

    /// Reads `author`'s segments that may hold ops past what we hold; returns
    /// the first op that failed locally. Only names that parse are fetched,
    /// so junk costs a line in a listing, never a download.
    pub(crate) async fn pull_author(
        &self,
        author: &PublicKey,
        listing: Listing,
    ) -> Result<Option<anyhow::Error>> {
        // Older versions stored where they resumed; reading no longer uses it.
        self.state().store.forget_cursor(author)?;
        let held = self.state().store.last_seq(author)?;
        let offset = match listing {
            Listing::PastHeld => Some(listing_after(author, held)),
            Listing::Whole => None,
        };
        let listed = self.bucket().list_segments(author, offset.as_deref()).await?;
        let skipped = self.skipped_segments().clone();
        let worth_fetching = listed
            .into_iter()
            .filter(|object| !skipped.contains(object))
            .filter(|object| parse_segment_name(author, &object.key).is_some_and(|name| name.last > held))
            .collect();
        let mut segments = self.bucket().fetch_segments(author, worth_fetching);
        let mut local_failure = None;
        while let Some(segment) = segments.next().await? {
            let read = self.receive_segment(author, &segment.listed.key, &segment.envelopes)?;
            if !read.is_settled && !read.is_stopped {
                let mut skipped = self.skipped_segments();
                if skipped.len() < MAX_SKIPPED_SEGMENTS {
                    skipped.insert(segment.listed);
                }
            }
            if let Some(error) = read.local_failure {
                local_failure.get_or_insert(error);
            }
            if read.is_stopped {
                break;
            }
        }
        Ok(local_failure)
    }

    /// Whether it's time to list every author whole: older names sort by
    /// their first seq, so a segment from an older version that we hold part
    /// of sorts before what we hold, and only a whole listing finds it. Timed
    /// by the wall clock, saved, so a restart or sleep doesn't put it off; a
    /// clock set back makes it due at once.
    pub(crate) fn is_whole_listing_due(&self) -> Result<bool> {
        let Some(saved) = self.state().store.meta(LAST_WHOLE_LISTING_META)? else {
            return Ok(true);
        };
        let Ok(bytes) = <[u8; 8]>::try_from(saved.as_slice()) else {
            warn!("the time of the last whole listing is unreadable; listing everyone whole");
            return Ok(true);
        };
        let last_ms = i64::from_le_bytes(bytes);
        let now = now_ms()?;
        Ok(now < last_ms || now.saturating_sub(last_ms) >= WHOLE_LISTING_EVERY_MS)
    }

    /// As if the last whole listing had been at `last_ms`.
    #[cfg(test)]
    pub(crate) fn set_last_whole_listing(&self, last_ms: i64) {
        self.state()
            .store
            .set_meta(LAST_WHOLE_LISTING_META, &last_ms.to_le_bytes())
            .expect("the store is writable in tests");
    }

    /// Once per gap: if an author's next op went missing after we read past
    /// it (a parked op pushed out by the cap), fetch again what was skipped
    /// this run, and list their whole history (at most once an hour each:
    /// a forged parked op can fake a gap whenever their log moves). Returns
    /// the authors to read again, and how.
    pub(super) fn refetch_gaps(&self) -> Result<Vec<(PublicKey, Listing)>> {
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
                let mut listed_whole = self
                    .inner
                    .gap_whole_listings
                    .lock()
                    .expect("gap listing times lock poisoned by an earlier panic");
                let is_due = listed_whole.get(&author).is_none_or(|last| {
                    last.elapsed() >= Duration::from_millis(WHOLE_LISTING_EVERY_MS.unsigned_abs())
                });
                if is_due {
                    listed_whole.insert(author, tokio::time::Instant::now());
                    refetching.push((author, Listing::Whole));
                } else {
                    refetching.push((author, Listing::PastHeld));
                }
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
