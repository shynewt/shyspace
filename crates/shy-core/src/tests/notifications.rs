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
async fn every_message_alerts_by_default_and_mentions_only_narrows_it() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut changes = admin.subscribe();
    bob.send_message(&general, "just chatting", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(alerted_texts(&mut changes), vec!["just chatting".to_string()]);

    admin.set_notify_level(&general, NotifyLevel::Mentions).unwrap();
    bob.send_message(&general, "more chatting", None).unwrap();
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

fn status_alerting(alerts: NotifyLevel) -> crate::views::StatusUpdate<'static> {
    crate::views::StatusUpdate {
        emoji: "🎧",
        text: "Focus",
        until_ms: None,
        alerts,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_status_can_silence_alerts() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin.set_status(status_alerting(NotifyLevel::Nothing)).unwrap();
    let mut changes = admin.subscribe();
    bob.send_message(&general, "anyone?", None).unwrap();
    let mention = crate::mentions::member_link("Ada", &admin.my_id());
    bob.send_message(&general, &format!("{mention} urgent"), None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(alerted_texts(&mut changes).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_status_can_let_only_mentions_through() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin.set_status(status_alerting(NotifyLevel::Mentions)).unwrap();
    let mut changes = admin.subscribe();
    bob.send_message(&general, "anyone?", None).unwrap();
    let mention = crate::mentions::member_link("Ada", &admin.my_id());
    bob.send_message(&general, &format!("{mention} urgent"), None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(alerted_texts(&mut changes), vec!["@Ada urgent"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stricter_of_channel_and_status_wins() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin.set_notify_level(&general, NotifyLevel::Nothing).unwrap();
    admin.set_status(status_alerting(NotifyLevel::All)).unwrap();
    let mut changes = admin.subscribe();
    let mention = crate::mentions::member_link("Ada", &admin.my_id());
    bob.send_message(&general, &format!("{mention} hello"), None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(
        alerted_texts(&mut changes).is_empty(),
        "a muted channel stays muted"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn my_other_devices_message_raises_no_alert_and_no_unread() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    bob.set_notify_level(&general, NotifyLevel::All).unwrap();
    let mut changes = bob.subscribe();

    phone.send_message(&general, "from my phone", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    assert!(alerted_texts(&mut changes).is_empty());
    let channel = bob
        .channels()
        .unwrap()
        .into_iter()
        .find(|channel| channel.id == general)
        .unwrap();
    assert_eq!(channel.unread, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mention_from_my_own_other_device_is_not_an_inbox_item() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;

    let mention = mentions::member_link("Bob", &bob.my_id());
    phone
        .send_message(&general, &format!("note to {mention}"), None)
        .unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    assert!(bob.inbox().unwrap().attention.is_empty());
    let shown = bob.messages(&general, None, 50).unwrap();
    assert!(shown.iter().all(|message| !message.mentions_me));

    // A mention of Bob by someone else reaches every one of his devices.
    admin
        .send_message(&general, &format!("ask {mention}"), None)
        .unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    for device in [&bob, &phone] {
        assert_eq!(device.inbox().unwrap().attention.len(), 1);
        let asked = device
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .find(|message| message.text.starts_with("ask"))
            .unwrap();
        assert!(asked.mentions_me);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_alert_from_a_linked_device_names_its_person() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    let mut changes = admin.subscribe();

    phone.send_message(&general, "from the phone", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    let mut named = Vec::new();
    while let Ok(change) = changes.try_recv() {
        if let Change::Alert {
            author, author_name, ..
        } = change
        {
            named.push((author, author_name));
        }
    }
    assert_eq!(named, [(bob.my_id(), "Bob".to_string())]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_status_set_on_one_device_quiets_alerts_on_the_others() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    phone.set_status(status_alerting(NotifyLevel::Nothing)).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let mut changes = bob.subscribe();

    admin.send_message(&general, "anyone?", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    assert!(alerted_texts(&mut changes).is_empty());
    let shown = admin
        .members()
        .unwrap()
        .into_iter()
        .find(|member| member.id == bob.my_id())
        .unwrap();
    assert!(shown.status.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_status_set_on_my_laptop_quiets_alerts_on_my_phone() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    bob.set_status(status_alerting(NotifyLevel::Nothing)).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let mut changes = phone.subscribe();

    admin.send_message(&general, "anyone?", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    assert!(alerted_texts(&mut changes).is_empty());
}
