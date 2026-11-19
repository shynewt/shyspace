//! Moving envelopes between the local log, the bucket, and online peers.

use anyhow::Result;
use tracing::warn;

use crate::workspace::Workspace;

impl Workspace {
    /// Reads new segments from everyone who was ever a member: a newcomer
    /// also needs the history of people removed before they joined. Our own
    /// log is included so a restored device learns what it already wrote.
    pub(crate) async fn pull(&self) -> Result<()> {
        self.refetch_gaps()?;
        let authors = self.state().store.everyone_ever()?;
        for author in authors {
            let cursor = self.state().store.cursor(&author)?;
            for (key, envelopes) in self.bucket().segments_after(&author, cursor.as_deref()).await? {
                for envelope in envelopes {
                    if envelope.author != author {
                        warn!("segment {key} contains an op by someone else; skipping it");
                        continue;
                    }
                    if let Err(error) = self.receive(envelope) {
                        warn!("dropping op from {key}: {error:#}");
                    }
                }
                self.state().store.set_cursor(&author, &key)?;
            }
        }
        Ok(())
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

    /// Uploads our ops that aren't in the bucket yet, as one segment.
    pub(super) async fn push(&self) -> Result<()> {
        let me = self.me();
        let unpushed = self.state().store.unpushed(&me)?;
        let Some(last) = unpushed.last().map(|envelope| envelope.seq) else {
            return Ok(());
        };
        self.bucket().put_segment(&unpushed).await?;
        self.state().store.mark_pushed(&me, last)?;
        Ok(())
    }

    /// Refreshes who may gossip with us, then connects to members online.
    pub(super) async fn join_peers(&self) -> Result<()> {
        let Some(net) = &self.inner.net else {
            return Ok(());
        };
        net.set_members(&self.current_members()?);
        net.join(self.bucket().peers().await?).await
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
