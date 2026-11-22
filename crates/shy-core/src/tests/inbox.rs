//! The inbox: what needs your attention across channels and threads.

use super::support::*;
use crate::{mentions, views::InboxReason};

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
