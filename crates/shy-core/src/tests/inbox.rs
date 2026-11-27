//! The inbox: what needs your attention across channels and threads.

use super::support::*;
use crate::{
    Workspace, mentions,
    views::{InboxReason, NotifyLevel},
};

#[tokio::test(flavor = "multi_thread")]
async fn mentions_and_followed_thread_replies_need_attention_newest_first() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let random = bob.create_channel("random").unwrap();
    let question = admin.send_message(&general, "who reviews?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    bob.send_message(&general, "plain chatter", None).unwrap();
    bob.send_message(&general, "I can", Some(question.clone()))
        .unwrap();
    let mention = mentions::member_link("Ada", &admin.my_id());
    bob.send_message(&random, &format!("{mention} look"), None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let inbox = admin.inbox().unwrap();
    let items: Vec<_> = inbox
        .attention
        .iter()
        .map(|item| (item.message.text.clone(), item.reason, item.channel_name.clone()))
        .collect();
    assert_eq!(
        items,
        vec![
            (
                format!("{mention} look"),
                InboxReason::Mention,
                "random".to_string()
            ),
            (
                "I can".to_string(),
                InboxReason::ThreadReply,
                "general".to_string()
            ),
        ]
    );
    assert_eq!(inbox.attention[1].thread_root.as_deref(), Some(question.as_str()));
    let unread: Vec<_> = inbox
        .channels
        .iter()
        .map(|c| (c.name.clone(), c.unread))
        .collect();
    assert!(
        unread.contains(&("general".to_string(), 1)) && unread.contains(&("random".to_string(), 1)),
        "{unread:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn marking_everything_read_empties_the_inbox() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "thoughts?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    bob.send_message(&general, "yes", Some(question)).unwrap();
    bob.send_message(&general, "also this", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(!admin.inbox().unwrap().attention.is_empty());

    admin.mark_all_read().unwrap();
    let inbox = admin.inbox().unwrap();
    assert!(inbox.attention.is_empty() && inbox.channels.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_inbox_shows_only_the_newest_hundred() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let random = bob.create_channel("random").unwrap();
    let mention = mentions::member_link("Ada", &admin.my_id());
    for number in 0..105 {
        let channel = if number % 2 == 0 { &general } else { &random };
        bob.send_message(channel, &format!("{mention} number {number}"), None)
            .unwrap();
    }
    sync_all(&[&admin, &bob]).await;

    let texts: Vec<String> = admin
        .inbox()
        .unwrap()
        .attention
        .into_iter()
        .map(|item| item.message.text)
        .collect();
    let newest_first: Vec<String> = (5..105)
        .rev()
        .map(|number| format!("{mention} number {number}"))
        .collect();
    assert_eq!(texts, newest_first);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_muted_channel_waits_in_the_inbox_only_when_it_mentions_you() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let quiet = bob.create_channel("quiet").unwrap();
    let ping = bob.create_channel("ping").unwrap();
    sync_all(&[&admin, &bob]).await;
    admin.set_notify_level(&quiet, NotifyLevel::Nothing).unwrap();
    admin.set_notify_level(&ping, NotifyLevel::Nothing).unwrap();
    sync_all(&[&admin, &bob]).await;
    bob.send_message(&quiet, "chatter", None).unwrap();
    let mention = mentions::member_link("Ada", &admin.my_id());
    bob.send_message(&ping, &format!("{mention} look"), None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let waiting: Vec<_> = admin
        .inbox()
        .unwrap()
        .channels
        .into_iter()
        .map(|channel| channel.name)
        .collect();

    assert!(waiting.contains(&"ping".to_string()), "{waiting:?}");
    assert!(!waiting.contains(&"quiet".to_string()), "{waiting:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn marking_a_channel_read_reads_it_up_to_its_newest_message() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "one", None).unwrap();
    bob.send_message(&general, "two", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(unread_in(&admin, &general), 2);

    admin.mark_channel_read(&general).unwrap();

    assert_eq!(unread_in(&admin, &general), 0);
    assert!(admin.inbox().unwrap().channels.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn marking_an_empty_channel_read_does_nothing() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let quiet = admin.create_channel("quiet").unwrap();

    admin.mark_channel_read(&quiet).unwrap();

    assert_eq!(unread_in(&admin, &quiet), 0);
}

fn unread_in(member: &Workspace, channel: &str) -> u32 {
    member
        .channels()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == channel)
        .expect("the channel is listed")
        .unread
}

#[tokio::test(flavor = "multi_thread")]
async fn marking_an_unknown_channel_read_fails() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;

    assert!(admin.mark_channel_read("no-such-channel").is_err());
}
