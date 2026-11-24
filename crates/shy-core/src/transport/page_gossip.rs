//! Live traffic about one page (who is on it, where their cursors are) on a
//! gossip topic of its own, so only the members with that page open hear
//! it, and apps that predate it never see it. It shares the workspace's
//! endpoint, so the members-only gate covers it like everything else.

use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};

use anyhow::Result;
use bytes::Bytes;
use futures_lite::StreamExt;
use iroh::PublicKey;
use iroh_gossip::{Event, GossipReceiver, GossipSender, TopicId};
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::warn;

use super::gossip::Net;
use crate::pages::cursors::{CursorUpdate, PageBeacon};

const PAGE_TOPIC_DOMAIN: &str = "shyspace page topic v1";
/// Taken from a page's topic in one second, from everyone together, before
/// anything is decoded or checked: enough for a crowded page, and a cap on
/// the work a flood can cause. Each member's own rate is checked after.
const MAX_PAGE_MESSAGES_PER_SECOND: u32 = 400;

/// What travels on a page's topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PageWire {
    Beacon(PageBeacon),
    Cursor(CursorUpdate),
}

/// The gossip topic for `page` in `workspace_id`.
pub fn page_topic(workspace_id: &str, page: &str) -> Result<[u8; 32]> {
    Ok(blake3::derive_key(
        PAGE_TOPIC_DOMAIN,
        &postcard::to_stdvec(&(workspace_id, page))?,
    ))
}

/// The page topic this device is on. Dropping it leaves the topic.
pub(super) struct OpenPage {
    topic: [u8; 32],
    sender: GossipSender,
    /// Where its listener hands what arrives.
    deliver: mpsc::Sender<PageWire>,
    listener: JoinHandle<()>,
}

impl Drop for OpenPage {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

impl Net {
    /// Joins `topic` (leaving any other page's), connecting to the members
    /// among `peers`, and hands what arrives to `deliver`; on the same topic,
    /// it just connects to new ones.
    pub async fn join_page(
        &self,
        topic: [u8; 32],
        peers: Vec<PublicKey>,
        deliver: mpsc::Sender<PageWire>,
    ) -> Result<()> {
        let peers = self.dialable(peers);
        // The same topic and queue: only connect to the new peers. Another
        // queue means whoever read the old one is gone, so listen afresh.
        let current = self
            .open_page()
            .as_ref()
            .filter(|open| open.topic == topic && open.deliver.same_channel(&deliver))
            .map(|open| open.sender.clone());
        if let Some(sender) = current {
            if !peers.is_empty() {
                sender.join_peers(peers).await?;
            }
            return Ok(());
        }
        self.leave_page();
        let (sender, receiver) = self
            .gossip
            .subscribe(TopicId::from_bytes(topic), peers)
            .await?
            .split();
        let listener = tokio::spawn(listen(receiver, deliver.clone()));
        *self.open_page() = Some(OpenPage {
            topic,
            sender,
            deliver,
            listener,
        });
        Ok(())
    }

    pub fn leave_page(&self) {
        self.open_page().take();
    }

    /// Sends to the members on the open page; nothing if none is open.
    pub async fn broadcast_page(&self, message: &PageWire) -> Result<()> {
        let Some(sender) = self.open_page().as_ref().map(|open| open.sender.clone()) else {
            return Ok(());
        };
        sender
            .broadcast(Bytes::from(postcard::to_stdvec(message)?))
            .await?;
        Ok(())
    }

    fn open_page(&self) -> std::sync::MutexGuard<'_, Option<OpenPage>> {
        self.page
            .lock()
            .expect("open page lock poisoned by an earlier panic")
    }
}

async fn listen(mut receiver: GossipReceiver, deliver: mpsc::Sender<PageWire>) {
    let limit = PageRate::default();
    while let Some(event) = receiver.next().await {
        let message = match event {
            Ok(Event::Received(message)) => message,
            Ok(_) => continue,
            Err(error) => {
                warn!("page gossip ended: {error}");
                break;
            }
        };
        let Ok(now_ms) = crate::ops::now_ms() else {
            continue;
        };
        if !limit.take(now_ms) {
            continue;
        }
        let Ok(wire) = postcard::from_bytes::<PageWire>(&message.content) else {
            continue;
        };
        // Never wait: a dropped cursor is replaced by the next.
        if let Err(mpsc::error::TrySendError::Closed(_)) = deliver.try_send(wire) {
            break;
        }
    }
}

/// Counts a page topic's messages per second.
#[derive(Default)]
struct PageRate {
    second: AtomicI64,
    count: AtomicU32,
}

impl PageRate {
    fn take(&self, now_ms: i64) -> bool {
        let second = now_ms.div_euclid(1000);
        if self.second.swap(second, Ordering::Relaxed) != second {
            self.count.store(0, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed) < MAX_PAGE_MESSAGES_PER_SECOND
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iroh::{Endpoint, SecretKey, address_lookup::memory::MemoryLookup, endpoint::presets};

    use super::*;

    async fn local_net(lookup: &MemoryLookup) -> (PublicKey, Net) {
        let (sender, _receiver) = mpsc::channel(16);
        let builder = Endpoint::builder(presets::Minimal).address_lookup(lookup.clone());
        let net = Net::start_with(builder, [7; 32], sender, None).await.unwrap();
        let id = net.endpoint().id();
        lookup.add_endpoint_info(net.endpoint().addr());
        (id, net)
    }

    /// Joins the page topic; returns what arrives on it.
    async fn join(net: &Net, topic: [u8; 32], peers: Vec<PublicKey>) -> mpsc::Receiver<PageWire> {
        let (deliver, inbox) = mpsc::channel(64);
        net.join_page(topic, peers, deliver).await.unwrap();
        inbox
    }

    /// Each one different: gossip delivers a message it has seen only once.
    fn cursor_from(identity: &SecretKey, page: &str) -> PageWire {
        PageWire::Cursor(CursorUpdate::signed(identity, "w", page, None, rand::random()).unwrap())
    }

    async fn next_page_message(inbox: &mut mpsc::Receiver<PageWire>, within: Duration) -> bool {
        tokio::time::timeout(within, inbox.recv())
            .await
            .ok()
            .flatten()
            .is_some()
    }

    /// Sends until it's heard (joining takes a moment), or gives up.
    async fn heard_after_sending(
        from: &Net,
        identity: &SecretKey,
        to: &mut mpsc::Receiver<PageWire>,
    ) -> bool {
        for _ in 0..20 {
            from.broadcast_page(&cursor_from(identity, "p")).await.unwrap();
            if next_page_message(to, Duration::from_millis(500)).await {
                return true;
            }
        }
        false
    }

    #[test]
    fn topics_differ_by_page_and_workspace() {
        let topic = page_topic("w", "p").unwrap();
        assert_ne!(topic, page_topic("w", "q").unwrap());
        assert_ne!(topic, page_topic("v", "p").unwrap());
        assert_ne!(topic, [7; 32]);
    }

    #[test]
    fn a_flood_is_cut_off_for_the_rest_of_its_second() {
        let rate = PageRate::default();
        let taken = (0..1000).filter(|_| rate.take(5_000)).count();
        assert_eq!(taken, MAX_PAGE_MESSAGES_PER_SECOND as usize);
        assert!(rate.take(6_000), "the next second starts afresh");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn members_on_a_page_hear_each_other_and_no_one_else_does() {
        let lookup = MemoryLookup::new();
        let (alice_id, alice) = local_net(&lookup).await;
        let bob_identity = SecretKey::generate();
        let (bob_id, bob) = local_net(&lookup).await;
        let (carol_id, carol) = local_net(&lookup).await;
        for net in [&alice, &bob, &carol] {
            net.set_members(&[alice_id, bob_id, carol_id]);
        }
        carol.join(vec![alice_id, bob_id]).await.unwrap();
        let topic = page_topic("w", "p").unwrap();
        let mut carol_other_page = join(&carol, page_topic("w", "q").unwrap(), vec![alice_id, bob_id]).await;
        let mut alice_inbox = join(&alice, topic, vec![bob_id, carol_id]).await;
        let _bob_inbox = join(&bob, topic, vec![alice_id, carol_id]).await;

        assert!(heard_after_sending(&bob, &bob_identity, &mut alice_inbox).await);
        assert!(
            !next_page_message(&mut carol_other_page, Duration::from_secs(2)).await,
            "a member on another page hears nothing of this one"
        );

        alice.leave_page();
        bob.broadcast_page(&cursor_from(&bob_identity, "p"))
            .await
            .unwrap();
        assert!(!next_page_message(&mut alice_inbox, Duration::from_secs(2)).await);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_outsider_cant_join_a_pages_topic() {
        let lookup = MemoryLookup::new();
        let (alice_id, alice) = local_net(&lookup).await;
        let (outsider_id, outsider) = local_net(&lookup).await;
        alice.set_members(&[alice_id]);
        outsider.set_members(&[alice_id, outsider_id]);
        let topic = page_topic("w", "p").unwrap();
        let mut alice_inbox = join(&alice, topic, vec![]).await;
        let _outsider_inbox = join(&outsider, topic, vec![alice_id]).await;

        assert!(!heard_after_sending(&outsider, &SecretKey::generate(), &mut alice_inbox).await);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_removed_member_is_cut_off_from_a_pages_topic() {
        let lookup = MemoryLookup::new();
        let (alice_id, alice) = local_net(&lookup).await;
        let (bob_id, bob) = local_net(&lookup).await;
        let bob_identity = SecretKey::generate();
        alice.set_members(&[alice_id, bob_id]);
        bob.set_members(&[alice_id, bob_id]);
        let topic = page_topic("w", "p").unwrap();
        let mut alice_inbox = join(&alice, topic, vec![bob_id]).await;
        let _bob_inbox = join(&bob, topic, vec![alice_id]).await;
        assert!(heard_after_sending(&bob, &bob_identity, &mut alice_inbox).await);

        alice.set_members(&[alice_id]);
        tokio::time::sleep(Duration::from_millis(500)).await;
        while alice_inbox.try_recv().is_ok() {}
        let (again, _) = mpsc::channel(1);
        bob.join_page(topic, vec![alice_id], again).await.unwrap();

        assert!(!heard_after_sending(&bob, &bob_identity, &mut alice_inbox).await);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn joining_a_page_again_with_a_new_queue_keeps_hearing_it() {
        let lookup = MemoryLookup::new();
        let (alice_id, alice) = local_net(&lookup).await;
        let (bob_id, bob) = local_net(&lookup).await;
        let bob_identity = SecretKey::generate();
        alice.set_members(&[alice_id, bob_id]);
        bob.set_members(&[alice_id, bob_id]);
        let topic = page_topic("w", "p").unwrap();
        let first = join(&alice, topic, vec![bob_id]).await;
        let _bob_inbox = join(&bob, topic, vec![alice_id]).await;
        let mut first = first;
        assert!(heard_after_sending(&bob, &bob_identity, &mut first).await);

        drop(first);
        let mut second = join(&alice, topic, vec![bob_id]).await;

        assert!(heard_after_sending(&bob, &bob_identity, &mut second).await);
    }
}
