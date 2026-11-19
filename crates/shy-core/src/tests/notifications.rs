//! Which arriving messages deserve an alert, by each channel's level.

use super::support::*;
use crate::{
    crypto, mentions,
    ops::{Op, OpBody, new_owned_id, now_ms},
    views::{Change, NotifyLevel},
};

fn alerted_texts(changes: &mut tokio::sync::broadcast::Receiver<Change>) -> Vec<String> {
    let mut texts = Vec::new();
    while let Ok(change) = changes.try_recv() {
        if let Change::Alert { preview, .. } = change {
            texts.push(preview);
        }
    }
    texts
}

#[tokio::test(flavor = "multi_thread")]
async fn by_default_only_mentions_alert() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut changes = admin.subscribe();
    bob.send_message(&general, "just chatting", None).unwrap();
    let mention = mentions::member_link("Ada", &admin.my_id());
    bob.send_message(&general, &format!("{mention} need you"), None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(alerted_texts(&mut changes), vec!["@Ada need you".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn all_alerts_for_every_message_and_nothing_mutes_even_mentions() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let random = bob.create_channel("random").unwrap();
    sync_all(&[&admin, &bob]).await;
    admin.set_notify_level(&general, NotifyLevel::All).unwrap();
    admin.set_notify_level(&random, NotifyLevel::Nothing).unwrap();
    let mut changes = admin.subscribe();

    bob.send_message(&general, "lunch?", None).unwrap();
    let mention = mentions::member_link("Ada", &admin.my_id());
    bob.send_message(&random, &format!("{mention} muted"), None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(alerted_texts(&mut changes), vec!["lunch?".to_string()]);
    let levels: Vec<_> = admin
        .channels()
        .unwrap()
        .into_iter()
        .map(|channel| channel.notify)
        .collect();
    assert!(levels.contains(&NotifyLevel::All) && levels.contains(&NotifyLevel::Nothing));
}

#[tokio::test(flavor = "multi_thread")]
async fn old_messages_caught_up_later_do_not_alert() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin.set_notify_level(&general, NotifyLevel::All).unwrap();
    let mut changes = admin.subscribe();
    let (epoch, key) = current_key(&bob);
    let an_hour_ago = now_ms().unwrap() - 60 * 60 * 1000;
    let body = OpBody {
        lamport: 9_000,
        timestamp_ms: an_hour_ago,
        op: Op::Message {
            id: new_owned_id(&bob.me()),
            channel: general,
            text: "from earlier".into(),
            reply_to: None,
        },
    };
    let seq = bob.state().store.next_seq(&bob.me()).unwrap();
    admin
        .receive(crypto::seal(&bob.inner.identity, &key, epoch, seq, &body).unwrap())
        .unwrap();

    assert!(alerted_texts(&mut changes).is_empty());
}
