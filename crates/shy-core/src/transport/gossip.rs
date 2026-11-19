//! Live delivery between online members over iroh gossip. Anything sent here
//! is already encrypted and signed; gossip only makes it arrive sooner than the
//! bucket would.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use anyhow::Result;
use bytes::Bytes;
use futures_lite::StreamExt;
use iroh::{Endpoint, PublicKey, SecretKey, endpoint::presets, protocol::Router};
use iroh_gossip::{Event, Gossip, GossipSender, TopicId};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::warn;

use crate::ops::Envelope;

const MAX_MESSAGE_SIZE: usize = 256 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub enum Wire {
    Envelope(Envelope),
    /// "Something new is in the bucket": MLS changes, join requests, or ops
    /// too large for gossip. The nonce defeats gossip's duplicate filter.
    Nudge(u64),
}

pub struct Net {
    router: Router,
    sender: GossipSender,
    neighbors: Arc<AtomicUsize>,
}

impl Net {
    pub async fn start(identity: SecretKey, topic: [u8; 32], incoming: mpsc::Sender<Wire>) -> Result<Self> {
        let endpoint = Endpoint::builder(presets::N0).secret_key(identity).bind().await?;
        let gossip = Gossip::builder()
            .max_message_size(MAX_MESSAGE_SIZE)
            .spawn(endpoint.clone());
        let router = Router::builder(endpoint)
            .accept(iroh_gossip::ALPN, gossip.clone())
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
                            if incoming.send(wire).await.is_err() {
                                break;
                            }
                        }
                        Err(error) => warn!("ignoring malformed gossip message: {error}"),
                    },
                    Ok(Event::NeighborUp(_)) => {
                        neighbor_count.fetch_add(1, Ordering::Relaxed);
                        // A peer that just appeared may have written while we were apart.
                        let _ = incoming.send(Wire::Nudge(0)).await;
                    }
                    Ok(Event::NeighborDown(_)) => {
                        neighbor_count.fetch_sub(1, Ordering::Relaxed);
                    }
                    Ok(Event::Lagged) => {
                        let _ = incoming.send(Wire::Nudge(0)).await;
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
        })
    }

    pub fn neighbors(&self) -> usize {
        self.neighbors.load(Ordering::Relaxed)
    }

    pub async fn join(&self, peers: Vec<PublicKey>) -> Result<()> {
        let own_id = self.router.endpoint().id();
        let peers: Vec<_> = peers.into_iter().filter(|peer| *peer != own_id).collect();
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
