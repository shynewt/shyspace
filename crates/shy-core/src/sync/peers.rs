//! Whom to dial over gossip. Every member announces itself in the bucket when
//! it starts and every few hours after; members who haven't for a day are
//! taken to be away and left alone, and nobody is dialed more often than
//! every few minutes unless they announce again (they just came back).

use std::collections::HashMap;

use anyhow::Result;
use iroh::PublicKey;
use tracing::warn;

use crate::{ops::now_ms, transport::bucket::PeerAnnouncement, workspace::Workspace};

/// How often a member online for long re-announces itself.
pub(crate) const ANNOUNCE_EVERY_MS: i64 = 6 * 60 * 60 * 1000;
/// An announcement older than this is from someone not around any more.
const FRESH_FOR_MS: i64 = 24 * 60 * 60 * 1000;
const REDIAL_AFTER_MS: i64 = 10 * 60 * 1000;

/// The peers worth dialing now, given when each was last dialed.
pub(crate) fn peers_to_dial(
    announcements: &[PeerAnnouncement],
    last_dialed_ms: &HashMap<PublicKey, i64>,
    now_ms: i64,
) -> Vec<PublicKey> {
    announcements
        .iter()
        .filter(|announcement| now_ms - announcement.announced_ms < FRESH_FOR_MS)
        .filter(|announcement| match last_dialed_ms.get(&announcement.peer) {
            None => true,
            Some(&dialed_ms) => {
                announcement.announced_ms > dialed_ms || now_ms - dialed_ms >= REDIAL_AFTER_MS
            }
        })
        .map(|announcement| announcement.peer)
        .collect()
}

impl Workspace {
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
    use iroh::SecretKey;

    use super::*;

    const HOUR_MS: i64 = 60 * 60 * 1000;
    const NOW_MS: i64 = 100 * 24 * HOUR_MS;

    fn announced(peer: PublicKey, hours_ago: i64) -> PeerAnnouncement {
        PeerAnnouncement {
            peer,
            announced_ms: NOW_MS - hours_ago * HOUR_MS,
        }
    }

    #[test]
    fn someone_away_for_a_day_is_left_alone() {
        let [here, away] = [SecretKey::generate().public(), SecretKey::generate().public()];
        let dial = peers_to_dial(
            &[announced(here, 2), announced(away, 30)],
            &HashMap::new(),
            NOW_MS,
        );
        assert_eq!(dial, [here]);
    }

    #[test]
    fn a_peer_is_redialed_after_a_while_or_at_once_when_they_announce_again() {
        let peer = SecretKey::generate().public();
        let dialed_a_minute_ago = HashMap::from([(peer, NOW_MS - 60_000)]);
        assert!(peers_to_dial(&[announced(peer, 2)], &dialed_a_minute_ago, NOW_MS).is_empty());

        let dialed_long_ago = HashMap::from([(peer, NOW_MS - 11 * 60_000)]);
        assert_eq!(
            peers_to_dial(&[announced(peer, 2)], &dialed_long_ago, NOW_MS),
            [peer]
        );

        let came_back = PeerAnnouncement {
            peer,
            announced_ms: NOW_MS - 1_000,
        };
        assert_eq!(peers_to_dial(&[came_back], &dialed_a_minute_ago, NOW_MS), [peer]);
    }
}
