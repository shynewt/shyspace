//! Threads: replies live beside the channel, under the message they answer.

use super::support::*;
use crate::{mentions, views::Change};

fn texts(messages: Vec<crate::views::Message>) -> Vec<String> {
    messages.into_iter().map(|message| message.text).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn replies_live_in_the_thread_and_the_channel_shows_a_summary() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "release today?", None).unwrap();
    let answer = bob
        .send_message(&general, "yes, after lunch", Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    // A reply to a reply joins the same thread.
    admin.send_message(&general, "great", Some(answer)).unwrap();
    sync_all(&[&admin, &bob]).await;

    let channel = bob.messages(&general, None, 50).unwrap();
    let root = channel.iter().find(|message| message.id == question).unwrap();
    assert_eq!(root.reply_count, 2);
    assert!(root.last_reply_ms.is_some());
    assert!(
        channel.iter().all(|message| message.reply_to.is_none()),
        "replies stay out of the channel"
    );
    assert_eq!(
        texts(bob.thread(&question).unwrap()),
        vec!["release today?", "yes, after lunch", "great"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_whose_message_is_gone_shows_in_the_channel() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let doomed = bob.send_message(&general, "oops", None).unwrap();
    admin
        .send_message(&general, "what was that?", Some(doomed.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    bob.delete_message(&doomed).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(texts(admin.messages(&general, None, 50).unwrap()).contains(&"what was that?".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn you_follow_threads_you_start_or_join_and_their_new_replies_count() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &carol]).await;
    let question = admin.send_message(&general, "who's on call?", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    bob.send_message(&general, "me", Some(question.clone())).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    let root_for = |workspace: &crate::Workspace| {
        workspace
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .find(|m| m.id == question)
            .unwrap()
    };
    assert!(root_for(&admin).is_following, "you follow threads you start");
    assert!(root_for(&bob).is_following, "and threads you reply in");
    assert!(!root_for(&carol).is_following);
    assert_eq!(root_for(&admin).unread_replies, 1);

    let latest = admin.thread(&question).unwrap().pop().unwrap();
    admin.mark_thread_read(&question, &latest.id).unwrap();
    assert_eq!(root_for(&admin).unread_replies, 0);

    carol.set_following(&question, true).unwrap();
    assert!(root_for(&carol).is_following);
    admin.set_following(&question, false).unwrap();
    assert!(!root_for(&admin).is_following);
}

#[tokio::test(flavor = "multi_thread")]
async fn replies_in_followed_threads_alert_and_do_not_count_as_channel_unread() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "thoughts?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let mut changes = admin.subscribe();
    bob.send_message(&general, "looks good", Some(question.clone()))
        .unwrap();
    bob.send_message(&general, "unrelated reply", Some(question))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let mut alerts = 0;
    while let Ok(change) = changes.try_recv() {
        alerts += usize::from(matches!(change, Change::Alert { .. }));
    }
    assert_eq!(alerts, 2, "a thread you started alerts you");
    let channel = admin
        .channels()
        .unwrap()
        .into_iter()
        .find(|c| c.id == general)
        .unwrap();
    assert_eq!(
        channel.unread, 0,
        "thread replies stay out of the channel's count"
    );

    let mention = mentions::member_link("Ada", &admin.my_id());
    let other = bob.send_message(&general, "side topic", None).unwrap();
    bob.send_message(&general, &format!("{mention} see here"), Some(other))
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let channel = admin
        .channels()
        .unwrap()
        .into_iter()
        .find(|c| c.id == general)
        .unwrap();
    assert_eq!(channel.unread_mentions, 1, "a mention in any thread still counts");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_link_finds_the_message_and_its_thread() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "release?", None).unwrap();
    let answer = admin
        .send_message(&general, "friday", Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let top = bob.linked_message(&question).unwrap().unwrap();
    assert_eq!((top.message.text.as_str(), top.thread_root), ("release?", None));
    let reply = bob.linked_message(&answer).unwrap().unwrap();
    assert_eq!(reply.thread_root.as_deref(), Some(question.as_str()));

    admin.delete_message(&answer).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(
        bob.linked_message(&answer).unwrap().is_none(),
        "a deleted message's link leads nowhere"
    );
}
