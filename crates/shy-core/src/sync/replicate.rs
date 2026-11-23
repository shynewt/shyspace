//! Moving envelopes between the local log, the bucket, and online peers.

use anyhow::Result;
use tracing::warn;

use super::{
    peers::peers_to_dial,
    receive::{LocalFailure, NotKept},
};
use crate::{ops::now_ms, transport::bucket::split_into_segments, workspace::Workspace};

impl Workspace {
    /// Reads new segments from everyone who was ever a member: a newcomer
    /// also needs the history of people removed before they joined. Our own
    /// log is included so a restored device learns what it already wrote.
    /// An op that fails locally is parked and doesn't hold the cursor back,
    /// unless it couldn't even be parked; the first such error is returned
    /// at the end.
    pub(crate) async fn pull(&self) -> Result<()> {
        self.refetch_gaps()?;
        let mut local_failure = None;
        let authors = self.state().store.everyone_ever()?;
        'authors: for author in authors {
            let cursor = self.state().store.cursor(&author)?;
            let mut segments = self.bucket().segments_after(&author, cursor.as_deref()).await?;
            while let Some((key, envelopes)) = segments.next().await? {
                for envelope in envelopes {
                    if envelope.author != author {
                        warn!("segment {key} contains an op by someone else; skipping it");
                        continue;
                    }
                    match self.receive(envelope) {
                        Ok(()) => {}
                        Err(error) if error.downcast_ref::<NotKept>().is_some() => {
                            // The cursor stays before this segment, so the
                            // next pull reads the op again.
                            local_failure.get_or_insert(error);
                            continue 'authors;
                        }
                        Err(error) if error.downcast_ref::<LocalFailure>().is_some() => {
                            local_failure.get_or_insert(error);
                        }
                        Err(error) => warn!("dropping op from {key}: {error:#}"),
                    }
                }
                self.state().store.set_cursor(&author, &key)?;
            }
        }
        local_failure.map_or(Ok(()), Err)
    }

    /// Once per gap: if an author's next op went missing after we read past
    /// it, read their segments again from the start.
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
            }
        }
        Ok(())
    }

    /// Uploads our ops that aren't in the bucket yet, in as few segments as
    /// others can still read.
    pub(crate) async fn push(&self) -> Result<()> {
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
