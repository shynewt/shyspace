//! Live delivery between online members over iroh gossip. Anything sent here
//! is already encrypted and signed; gossip only makes it arrive sooner than the
//! bucket would. Only current members may connect.

use std::{
    collections::HashSet,
    sync::{
        Arc, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::Result;
use bytes::Bytes;
use futures_lite::StreamExt;
use iroh::{
    Endpoint, PublicKey, SecretKey,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use iroh_gossip::{Event, Gossip, GossipSender, TopicId};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{ops::Envelope, workspace::Heartbeat};

const MAX_MESSAGE_SIZE: usize = 256 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub enum Wire {
    Envelope(Envelope),
    /// "Something new is in the bucket": MLS changes, join requests, or ops
    /// too large for gossip. The nonce defeats gossip's duplicate filter.
    Nudge(u64),
    /// A signed "I'm here"; checked by the receiver, never stored.
    Presence(Heartbeat),
}

/// What gossip hands the workspace: messages from peers, and local events a
/// peer can't fake.
#[derive(Debug)]
pub enum Incoming {
    Message(Wire),
    /// A peer connected: it may have written while we were apart, and it
    /// hasn't heard our presence yet.
    NeighborUp,
    /// Messages were missed; the bucket has them.
    Lagged,
}

type Allowlist = Arc<RwLock<HashSet<PublicKey>>>;

/// Refuses gossip connections from anyone who isn't a current member, so
/// outsiders can't watch traffic or flood us with junk to verify.
#[derive(Debug, Clone)]
struct MembersOnly {
    gossip: Gossip,
    allowed: Allowlist,
}

impl ProtocolHandler for MembersOnly {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let is_member = self
            .allowed
            .read()
            .is_ok_and(|allowed| allowed.contains(&connection.remote_id()));
        if !is_member {
            return Err(AcceptError::from_err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "not a member of this workspace",
            )));
        }
        self.gossip.accept(connection).await
    }

    async fn shutdown(&self) {
        if let Err(error) = self.gossip.shutdown().await {
            warn!("gossip shutdown: {error}");
        }
    }
}

pub struct Net {
    router: Router,
    sender: GossipSender,
    neighbors: Arc<AtomicUsize>,
    allowed: Allowlist,
}

impl Net {
    pub async fn start(
        identity: SecretKey,
        topic: [u8; 32],
        incoming: mpsc::Sender<Incoming>,
    ) -> Result<Self> {
        let endpoint = Endpoint::builder(presets::N0).secret_key(identity).bind().await?;
        Self::start_on(endpoint, topic, incoming).await
    }

    /// Runs gossip on an endpoint that's already bound. Starts allowing no one
    /// until `set_members` is called.
    pub(crate) async fn start_on(
        endpoint: Endpoint,
        topic: [u8; 32],
        incoming: mpsc::Sender<Incoming>,
    ) -> Result<Self> {
        let gossip = Gossip::builder()
            .max_message_size(MAX_MESSAGE_SIZE)
            .spawn(endpoint.clone());
        let allowed = Allowlist::default();
        let handler = MembersOnly {
            gossip: gossip.clone(),
            allowed: allowed.clone(),
        };
        let router = Router::builder(endpoint)
            .accept(iroh_gossip::ALPN, handler)
            .spawn();
        let (sender, mut receiver) = gossip
            .subscribe(TopicId::from_bytes(topic), Vec::new())
            .await?
            .split();

        let neighbors = Arc::new(AtomicUsize::new(0));
        let neighbor_count = neighbors.clone();
        tokio::spawn(async move {
            while let Some(event) = receiver.next().await {
                match event {
                    Ok(Event::Received(message)) => match postcard::from_bytes::<Wire>(&message.content) {
                        Ok(wire) => {
                            if incoming.send(Incoming::Message(wire)).await.is_err() {
                                break;
                            }
                        }
                        Err(error) => warn!("ignoring malformed gossip message: {error}"),
                    },
                    Ok(Event::NeighborUp(_)) => {
                        neighbor_count.fetch_add(1, Ordering::Relaxed);
                        if incoming.send(Incoming::NeighborUp).await.is_err() {
                            break;
                        }
                    }
                    Ok(Event::NeighborDown(_)) => {
                        neighbor_count.fetch_sub(1, Ordering::Relaxed);
                    }
                    Ok(Event::Lagged) => {
                        if incoming.send(Incoming::Lagged).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        warn!("gossip stream ended: {error}");
                        break;
                    }
                }
            }
        });
        Ok(Self {
            router,
            sender,
            neighbors,
            allowed,
        })
    }

    pub fn neighbors(&self) -> usize {
        self.neighbors.load(Ordering::Relaxed)
    }

    /// Replaces who may connect to us and whom we connect to.
    pub fn set_members(&self, members: &[PublicKey]) {
        match self.allowed.write() {
            Ok(mut allowed) => *allowed = members.iter().copied().collect(),
            Err(_) => warn!("gossip allowlist lock poisoned; keeping the previous members"),
        }
    }

    /// Connects to the given peers, skipping ourselves and non-members.
    pub async fn join(&self, peers: Vec<PublicKey>) -> Result<()> {
        let own_id = self.router.endpoint().id();
        let peers: Vec<_> = {
            let allowed = self
                .allowed
                .read()
                .map_err(|_| anyhow::anyhow!("gossip allowlist lock poisoned"))?;
            peers
                .into_iter()
                .filter(|peer| *peer != own_id && allowed.contains(peer))
                .collect()
        };
        if !peers.is_empty() {
            self.sender.join_peers(peers).await?;
        }
        Ok(())
    }

    /// Sends an envelope live, or a nudge when it is too large for gossip.
    pub async fn broadcast(&self, envelope: Envelope) -> Result<()> {
        let encoded = postcard::to_stdvec(&Wire::Envelope(envelope))?;
        if encoded.len() <= self.sender.max_content_size() {
            self.sender.broadcast(Bytes::from(encoded)).await?;
            Ok(())
        } else {
            self.nudge().await
        }
    }

    pub async fn broadcast_presence(&self, heartbeat: Heartbeat) -> Result<()> {
        let encoded = postcard::to_stdvec(&Wire::Presence(heartbeat))?;
        self.sender.broadcast(Bytes::from(encoded)).await?;
        Ok(())
    }

    pub async fn nudge(&self) -> Result<()> {
        let encoded = postcard::to_stdvec(&Wire::Nudge(rand::random()))?;
        self.sender.broadcast(Bytes::from(encoded)).await?;
        Ok(())
    }

    pub async fn shutdown(&self) {
        if let Err(error) = self.router.shutdown().await {
            warn!("network shutdown: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iroh::address_lookup::memory::MemoryLookup;

    use super::*;

    async fn local_net(lookup: &MemoryLookup) -> (PublicKey, Net, mpsc::Receiver<Incoming>) {
        let endpoint = Endpoint::builder(presets::Minimal)
            .address_lookup(lookup.clone())
            .bind()
            .await
            .unwrap();
        lookup.add_endpoint_info(endpoint.addr());
        let id = endpoint.id();
        let (sender, receiver) = mpsc::channel(16);
        let net = Net::start_on(endpoint, [7; 32], sender).await.unwrap();
        (id, net, receiver)
    }

    async fn next_nonce(receiver: &mut mpsc::Receiver<Incoming>, within: Duration) -> Option<u64> {
        tokio::time::timeout(within, async {
            loop {
                match receiver.recv().await {
                    Some(Incoming::Message(Wire::Nudge(nonce))) => return Some(nonce),
                    Some(_) => continue,
                    None => return None,
                }
            }
        })
        .await
        .ok()
        .flatten()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn members_hear_each_other_and_outsiders_are_refused() {
        let lookup = MemoryLookup::new();
        let (alice_id, alice, _alice_inbox) = local_net(&lookup).await;
        let (bob_id, bob, mut bob_inbox) = local_net(&lookup).await;
        let (outsider_id, outsider, mut outsider_inbox) = local_net(&lookup).await;
        alice.set_members(&[alice_id, bob_id]);
        bob.set_members(&[alice_id, bob_id]);
        outsider.set_members(&[alice_id, bob_id, outsider_id]);

        bob.join(vec![alice_id]).await.unwrap();
        outsider.join(vec![alice_id, bob_id]).await.unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        alice.nudge().await.unwrap();

        assert!(
            next_nonce(&mut bob_inbox, Duration::from_secs(10))
                .await
                .is_some()
        );
        assert!(
            next_nonce(&mut outsider_inbox, Duration::from_secs(3))
                .await
                .is_none()
        );
        assert_eq!(alice.neighbors(), 1, "only Bob should be connected to Alice");
    }
}
