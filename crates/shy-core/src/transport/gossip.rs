//! Live delivery between online members over iroh gossip. Anything sent here
//! is already encrypted and signed; gossip only makes it arrive sooner than the
//! bucket would. Only current members may connect.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::Result;
use bytes::Bytes;
use futures_lite::StreamExt;
use iroh::{
    Endpoint, EndpointAddr, PublicKey, SecretKey,
    endpoint::{
        self, AfterHandshakeOutcome, BeforeConnectOutcome, Connection, EndpointHooks, VarInt,
        WeakConnectionHandle, presets,
    },
    protocol::{Router, RouterBuilder},
};
use iroh_gossip::{Event, Gossip, GossipSender, TopicId};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::warn;

use super::direct::{DIRECT_ALPN, Direct, DirectMessage};
use crate::{huddle::beacon::Beacon, ops::Envelope, sync::Hint, workspace::Heartbeat};

const MAX_MESSAGE_SIZE: usize = 256 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub enum Wire {
    Envelope(Envelope),
    /// "Something new is in the bucket": MLS changes, join requests, or ops
    /// too large for gossip. The nonce defeats gossip's duplicate filter.
    Nudge(u64),
    /// A signed "I'm here"; checked by the receiver, never stored.
    Presence(Heartbeat),
    /// A signed "my head is at this seq and version", sent after an upload.
    Hint(Hint),
    /// A public channel's huddle beacon; checked by the receiver, never stored.
    Huddle(Beacon),
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
    /// A message sent straight to us, and the device that sent it, as the
    /// connection proved.
    Direct {
        from: PublicKey,
        message: DirectMessage,
    },
}

/// Who may be connected, and the connections they have open with us.
#[derive(Debug, Default)]
struct Admitted {
    members: HashSet<PublicKey>,
    connections: HashMap<PublicKey, Vec<WeakConnectionHandle>>,
}

impl Admitted {
    /// Closes the connections of anyone no longer a member and forgets those already closed.
    fn close_non_members(&mut self) {
        let members = &self.members;
        self.connections.retain(|peer, handles| {
            if members.contains(peer) {
                handles.retain(|handle| handle.upgrade().is_some());
                return !handles.is_empty();
            }
            for connection in handles.iter().filter_map(WeakConnectionHandle::upgrade) {
                connection.close(NOT_A_MEMBER, b"not a member of this workspace");
            }
            false
        });
    }
}

const NOT_A_MEMBER: VarInt = VarInt::from_u32(403);

/// Only current members may be connected to us, in either direction, so
/// outsiders and removed members can't watch traffic or flood us with junk to
/// verify. Installed on the endpoint, so it sees every connection once its
/// handshake proves who the peer is.
#[derive(Debug, Clone, Default)]
struct MembersOnly(Arc<Mutex<Admitted>>);

impl MembersOnly {
    fn admitted(&self) -> MutexGuard<'_, Admitted> {
        self.0
            .lock()
            .expect("gossip members lock poisoned by an earlier panic")
    }

    fn is_member(&self, peer: &PublicKey) -> bool {
        self.admitted().members.contains(peer)
    }

    fn set_members(&self, members: &[PublicKey]) {
        let mut admitted = self.admitted();
        admitted.members = members.iter().copied().collect();
        admitted.close_non_members();
    }
}

impl EndpointHooks for MembersOnly {
    async fn before_connect<'a>(&'a self, remote: &'a EndpointAddr, _alpn: &'a [u8]) -> BeforeConnectOutcome {
        if self.is_member(&remote.id) {
            BeforeConnectOutcome::Accept
        } else {
            BeforeConnectOutcome::Reject
        }
    }

    /// Checks and records under one lock, so a removal either refuses this
    /// connection or finds it to close.
    async fn after_handshake<'a>(&'a self, connection: &'a Connection) -> AfterHandshakeOutcome {
        let peer = connection.remote_id();
        let mut admitted = self.admitted();
        if !admitted.members.contains(&peer) {
            return AfterHandshakeOutcome::Reject {
                error_code: NOT_A_MEMBER,
                reason: b"not a member of this workspace".to_vec(),
            };
        }
        let handles = admitted.connections.entry(peer).or_default();
        handles.retain(|handle| handle.upgrade().is_some());
        handles.push(connection.weak_handle());
        AfterHandshakeOutcome::Accept
    }
}

/// Protocols an application serves on the workspace's endpoint next to
/// gossip, such as huddle media. They get the same members-only admission:
/// only current members can open them, and a removed member's connections
/// close.
#[derive(Clone)]
pub struct ExtraProtocols(Arc<MountProtocols>);

/// Adds protocols to the router being built for `Endpoint`.
type MountProtocols = dyn Fn(&Endpoint, RouterBuilder) -> RouterBuilder + Send + Sync;

impl ExtraProtocols {
    pub fn new(mount: impl Fn(&Endpoint, RouterBuilder) -> RouterBuilder + Send + Sync + 'static) -> Self {
        Self(Arc::new(mount))
    }
}

pub struct Net {
    router: Router,
    sender: GossipSender,
    neighbors: Arc<AtomicUsize>,
    members_only: MembersOnly,
}

impl Net {
    pub async fn start(
        identity: SecretKey,
        topic: [u8; 32],
        incoming: mpsc::Sender<Incoming>,
        extra: Option<&ExtraProtocols>,
    ) -> Result<Self> {
        Self::start_with(
            Endpoint::builder(presets::N0).secret_key(identity),
            topic,
            incoming,
            extra,
        )
        .await
    }

    /// Binds `endpoint` and runs gossip on it. Allows no one until `set_members` is called.
    pub(crate) async fn start_with(
        endpoint: endpoint::Builder,
        topic: [u8; 32],
        incoming: mpsc::Sender<Incoming>,
        extra: Option<&ExtraProtocols>,
    ) -> Result<Self> {
        let members_only = MembersOnly::default();
        let endpoint = endpoint.hooks(members_only.clone()).bind().await?;
        let gossip = Gossip::builder()
            .max_message_size(MAX_MESSAGE_SIZE)
            .spawn(endpoint.clone());
        let mut router = Router::builder(endpoint.clone())
            .accept(iroh_gossip::ALPN, gossip.clone())
            .accept(DIRECT_ALPN, Direct::new(incoming.clone()));
        if let Some(extra) = extra {
            router = (extra.0)(&endpoint, router);
        }
        let router = router.spawn();
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
            members_only,
        })
    }

    pub fn neighbors(&self) -> usize {
        self.neighbors.load(Ordering::Relaxed)
    }

    /// Replaces who may be connected to us, closing the connections of anyone left out.
    pub fn set_members(&self, members: &[PublicKey]) {
        self.members_only.set_members(members);
    }

    /// Connects to the given peers, skipping ourselves and non-members.
    pub async fn join(&self, peers: Vec<PublicKey>) -> Result<()> {
        let own_id = self.router.endpoint().id();
        let peers: Vec<_> = peers
            .into_iter()
            .filter(|peer| *peer != own_id && self.members_only.is_member(peer))
            .collect();
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

    pub async fn broadcast_hint(&self, hint: Hint) -> Result<()> {
        let encoded = postcard::to_stdvec(&Wire::Hint(hint))?;
        self.sender.broadcast(Bytes::from(encoded)).await?;
        Ok(())
    }

    pub async fn broadcast_beacon(&self, beacon: Beacon) -> Result<()> {
        let encoded = postcard::to_stdvec(&Wire::Huddle(beacon))?;
        self.sender.broadcast(Bytes::from(encoded)).await?;
        Ok(())
    }

    /// Sends a message straight to one member's device.
    pub async fn send_direct(&self, to: PublicKey, message: &DirectMessage) -> Result<()> {
        super::direct::send_direct(self.router.endpoint(), to, message).await
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
        let (sender, receiver) = mpsc::channel(16);
        let builder = Endpoint::builder(presets::Minimal).address_lookup(lookup.clone());
        let net = Net::start_with(builder, [7; 32], sender, None).await.unwrap();
        let endpoint = net.router.endpoint();
        lookup.add_endpoint_info(endpoint.addr());
        (endpoint.id(), net, receiver)
    }

    fn beacon_from(identity: &SecretKey) -> crate::huddle::beacon::Beacon {
        use crate::huddle::beacon::{Beacon, BeaconBody, BeaconState, MEDIA_WIRE};
        let body = BeaconBody {
            member: identity.public(),
            endpoint: identity.public(),
            channel: "c".into(),
            session: [1; 16],
            seq: 0,
            sent_ms: 0,
            state: BeaconState::Left,
            media_wire: MEDIA_WIRE,
        };
        Beacon::sign(identity, "w", body).unwrap()
    }

    async fn next_direct(receiver: &mut mpsc::Receiver<Incoming>, within: Duration) -> Option<PublicKey> {
        tokio::time::timeout(within, async {
            loop {
                match receiver.recv().await {
                    Some(Incoming::Direct { from, .. }) => return Some(from),
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
    async fn direct_messages_arrive_one_after_another_with_their_sender() {
        let lookup = MemoryLookup::new();
        let (alice_id, alice, mut alice_inbox) = local_net(&lookup).await;
        let (bob_id, bob, _bob_inbox) = local_net(&lookup).await;
        alice.set_members(&[alice_id, bob_id]);
        bob.set_members(&[alice_id, bob_id]);
        let message = DirectMessage::Huddle(beacon_from(&SecretKey::generate()));
        for _ in 0..3 {
            bob.send_direct(alice_id, &message).await.unwrap();
            assert_eq!(
                next_direct(&mut alice_inbox, Duration::from_secs(5)).await,
                Some(bob_id)
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_outsider_cant_send_direct_messages() {
        let lookup = MemoryLookup::new();
        let (alice_id, alice, mut alice_inbox) = local_net(&lookup).await;
        let (outsider_id, outsider, _outsider_inbox) = local_net(&lookup).await;
        alice.set_members(&[alice_id]);
        outsider.set_members(&[alice_id, outsider_id]);
        let message = DirectMessage::Huddle(beacon_from(&SecretKey::generate()));
        assert!(outsider.send_direct(alice_id, &message).await.is_err());
        assert_eq!(next_direct(&mut alice_inbox, Duration::from_secs(2)).await, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_member_holding_connections_open_cant_block_the_others() {
        let lookup = MemoryLookup::new();
        let (alice_id, alice, mut alice_inbox) = local_net(&lookup).await;
        let (bob_id, bob, _bob_inbox) = local_net(&lookup).await;
        let (mallory_id, mallory, _mallory_inbox) = local_net(&lookup).await;
        for net in [&alice, &bob, &mallory] {
            net.set_members(&[alice_id, bob_id, mallory_id]);
        }
        // Mallory opens many connections and never finishes a message. Alice
        // refusing some of them, so they fail here, is the limit at work.
        let mut held = Vec::new();
        for _ in 0..100 {
            let connection = mallory
                .router
                .endpoint()
                .connect(alice_id, super::super::direct::DIRECT_ALPN)
                .await
                .unwrap();
            let Ok(mut stream) = connection.open_uni().await else {
                continue;
            };
            if stream.write_all(b"\x00").await.is_ok() {
                held.push((connection, stream));
            }
        }
        assert!(
            !held.is_empty(),
            "Mallory held no connection open, so nothing was tested"
        );
        let message = DirectMessage::Huddle(beacon_from(&SecretKey::generate()));
        bob.send_direct(alice_id, &message).await.unwrap();
        assert_eq!(
            next_direct(&mut alice_inbox, Duration::from_secs(5)).await,
            Some(bob_id)
        );
    }

    /// Echoes one message back: stands in for a protocol an app adds, like huddle media.
    #[derive(Debug, Clone)]
    struct Echo;

    const ECHO_ALPN: &[u8] = b"shyspace/test-echo/1";

    impl iroh::protocol::ProtocolHandler for Echo {
        async fn accept(&self, connection: Connection) -> Result<(), iroh::protocol::AcceptError> {
            let (mut send, mut recv) = connection.accept_bi().await?;
            let message = recv
                .read_to_end(64)
                .await
                .map_err(iroh::protocol::AcceptError::from_err)?;
            send.write_all(&message)
                .await
                .map_err(iroh::protocol::AcceptError::from_err)?;
            send.finish()?;
            connection.closed().await;
            Ok(())
        }
    }

    async fn echo_net(lookup: &MemoryLookup) -> (PublicKey, Net) {
        let (sender, _receiver) = mpsc::channel(16);
        let builder = Endpoint::builder(presets::Minimal).address_lookup(lookup.clone());
        let extra = ExtraProtocols::new(|_, router| router.accept(ECHO_ALPN, Echo));
        let net = Net::start_with(builder, [7; 32], sender, Some(&extra))
            .await
            .unwrap();
        let endpoint = net.router.endpoint();
        lookup.add_endpoint_info(endpoint.addr());
        (endpoint.id(), net)
    }

    async fn echo_through(from: &Net, to: PublicKey) -> anyhow::Result<Vec<u8>> {
        let connection = from.router.endpoint().connect(to, ECHO_ALPN).await?;
        let (mut send, mut recv) = connection.open_bi().await?;
        send.write_all(b"hello").await?;
        send.finish()?;
        Ok(recv.read_to_end(64).await?)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_added_protocol_is_open_to_members_only() {
        let lookup = MemoryLookup::new();
        let (alice_id, alice) = echo_net(&lookup).await;
        let (bob_id, bob) = echo_net(&lookup).await;
        let (outsider_id, outsider) = echo_net(&lookup).await;
        alice.set_members(&[alice_id, bob_id]);
        bob.set_members(&[alice_id, bob_id]);
        outsider.set_members(&[alice_id, outsider_id]);

        let echoed = tokio::time::timeout(Duration::from_secs(10), echo_through(&bob, alice_id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(echoed, b"hello");
        let refused = tokio::time::timeout(Duration::from_secs(10), echo_through(&outsider, alice_id))
            .await
            .unwrap();
        assert!(
            refused.is_err(),
            "someone who isn't a member opened the added protocol"
        );
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

    async fn wait_for_neighbors(net: &Net, expected: usize, within: Duration) -> bool {
        tokio::time::timeout(within, async {
            while net.neighbors() != expected {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .is_ok()
    }

    enum Dialer {
        Alice,
        Bob,
    }

    /// Alice removes Bob once they're connected; Carol stays. Carol never knew
    /// Bob, so anything Bob hears can only come through Alice.
    async fn removed_member_is_cut_off(dialer: Dialer) {
        let lookup = MemoryLookup::new();
        let (alice_id, alice, _alice_inbox) = local_net(&lookup).await;
        let (bob_id, bob, mut bob_inbox) = local_net(&lookup).await;
        let (carol_id, carol, mut carol_inbox) = local_net(&lookup).await;
        alice.set_members(&[alice_id, bob_id, carol_id]);
        bob.set_members(&[alice_id, bob_id]);
        carol.set_members(&[alice_id, carol_id]);
        match dialer {
            Dialer::Alice => alice.join(vec![bob_id]).await.unwrap(),
            Dialer::Bob => bob.join(vec![alice_id]).await.unwrap(),
        }
        carol.join(vec![alice_id]).await.unwrap();
        assert!(wait_for_neighbors(&alice, 2, Duration::from_secs(10)).await);

        alice.set_members(&[alice_id, carol_id]);

        assert!(
            wait_for_neighbors(&alice, 1, Duration::from_secs(5)).await,
            "only the removed member's connection should be closed"
        );
        alice.nudge().await.unwrap();
        assert!(
            next_nonce(&mut carol_inbox, Duration::from_secs(10))
                .await
                .is_some(),
            "members who stay must keep hearing each other"
        );
        assert!(
            next_nonce(&mut bob_inbox, Duration::from_secs(3)).await.is_none(),
            "a removed member must not hear live traffic"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_removed_member_who_connected_to_us_is_cut_off() {
        removed_member_is_cut_off(Dialer::Bob).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_removed_member_we_connected_to_is_cut_off() {
        removed_member_is_cut_off(Dialer::Alice).await;
    }
}
