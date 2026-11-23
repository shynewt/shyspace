//! Moving envelopes between the local log, the bucket, and online peers.

use anyhow::Result;
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

impl Workspace {
    /// Reads new segments from everyone who was ever a member: a newcomer
    /// also needs the history of people removed before they joined. Our own
    /// log is included so a restored device learns what it already wrote.
    /// Each author is listed from what we hold of their log, never from a
    /// stored bucket key, so objects others put in the bucket can't make us
    /// skip any of it. An op that fails locally is parked and doesn't hold
    /// reading back, unless it couldn't even be parked; the first such error
    /// is returned at the end.
    pub(crate) async fn pull(&self) -> Result<()> {
        self.refetch_gaps()?;
        let mut local_failure = None;
        let authors = self.state().store.everyone_ever()?;
        'authors: for author in authors {
            let held = self.state().store.last_seq(&author)?;
            let older_cursor = self.older_cursor(&author)?;
            let after_held = listing_after(&author, held);
            let offset = match &older_cursor {
                Some(cursor) if *cursor < after_held => cursor.clone(),
                _ => after_held,
            };
            let skipped = self.skipped_segments().clone();
            let mut segments = self
                .bucket()
                .segments_after(&author, &offset)
                .await?
                .without(&skipped);
            let mut has_news_in_older_names = false;
            while let Some(segment) = segments.next().await? {
                let held_before = self.state().store.last_seq(&author)?;
                let read = self.receive_segment(&author, &segment.listed.key, &segment.envelopes)?;
                let is_older_name =
                    parse_segment_name(&author, &segment.listed.key).is_some_and(|name| name.hash.is_none());
                if is_older_name && self.state().store.last_seq(&author)? > held_before {
                    has_news_in_older_names = true;
                }
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
                    continue 'authors;
                }
            }
            // Older names sort by their first seq, so one we hold part of sorts
            // before what we hold; until a pass from the older cursor finds
            // nothing more in them, keep listing from it.
            if older_cursor.is_some() && !has_news_in_older_names {
                self.state().store.forget_cursor(&author)?;
            }
        }
        local_failure.map_or(Ok(()), Err)
    }

    /// Where an older version resumed reading `author`, if it was a segment;
    /// anything else (junk an older version trusted) is dropped.
    fn older_cursor(&self, author: &iroh::PublicKey) -> Result<Option<String>> {
        let Some(cursor) = self.state().store.cursor(author)? else {
            return Ok(None);
        };
        if parse_segment_name(author, &cursor).is_some() {
            return Ok(Some(cursor));
        }
        warn!(
            "reading {author}'s history again: an older version resumed after {cursor}, which isn't a segment"
        );
        self.state().store.forget_cursor(author)?;
        Ok(None)
    }

    /// Once per gap: if an author's next op went missing after we read past
    /// it (a parked op pushed out by the cap), fetch their segments again
    /// rather than skip the ones read this run.
    fn refetch_gaps(&self) -> Result<()> {
        let state = self.state();
        let mut refetched = self
            .inner
            .refetched_gaps
            .lock()
            .expect("gap list lock poisoned by an earlier panic");
        for (author, next_seq) in state.store.authors_missing_next_op()? {
            if refetched.insert((author, next_seq)) {
                warn!("op {next_seq} from {author} went missing; reading their history again");
                state.store.forget_cursor(&author)?;
                let author_prefix = format!("ops/{}/", member_id(&author));
                self.skipped_segments()
                    .retain(|listed| !listed.key.starts_with(&author_prefix));
            }
        }
        Ok(())
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
        for segment in split_into_segments(&unpushed)? {
            let last = segment
                .last()
                .expect("split_into_segments never makes an empty segment")
                .seq;
            self.bucket().put_segment(segment).await?;
            self.state().store.mark_pushed(&me, last)?;
        }
        Ok(())
    }

    /// Refreshes who may gossip with us, then connects to members who seem
    /// to be around (see `peers_to_dial`).
    pub(super) async fn join_peers(&self) -> Result<()> {
        let Some(net) = &self.inner.net else {
            return Ok(());
        };
        net.set_members(&self.current_members()?);
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
