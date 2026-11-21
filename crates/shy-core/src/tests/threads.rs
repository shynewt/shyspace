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
async fn a_mention_in_a_thread_is_read_once_the_thread_is_read() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let icons = bob.send_message(&general, "new icons", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    admin.mark_read(&general, &icons).unwrap();
    let mention = mentions::member_link("Ada", &admin.my_id());
    let ask = bob
        .send_message(&general, &format!("{mention} dark mode?"), Some(icons.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let mentions_for_admin = || {
        let channel = admin
            .channels()
            .unwrap()
            .into_iter()
            .find(|c| c.id == general)
            .unwrap();
        let in_inbox = admin
            .inbox()
            .unwrap()
            .attention
            .iter()
            .filter(|item| item.message.id == ask)
            .count();
        (channel.unread_mentions, in_inbox)
    };
    assert_eq!(mentions_for_admin(), (1, 1));

    admin.mark_thread_read(&icons, &ask).unwrap();
    assert_eq!(
        mentions_for_admin(),
        (0, 0),
        "reading the thread reads the mention, with the channel's position unmoved"
    );
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

#[tokio::test(flavor = "multi_thread")]
async fn the_latest_message_includes_thread_replies() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "release?", None).unwrap();
    // Bob answers after seeing the question, as people do.
    sync_all(&[&admin, &bob]).await;
    bob.send_message(&general, "friday", Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let latest = admin.latest_message(&general).unwrap().unwrap();
    assert_eq!(latest.message.text, "friday");
    assert_eq!(latest.thread_root.as_deref(), Some(question.as_str()));
}

fn root_in(workspace: &crate::Workspace, channel: &str, root: &str) -> crate::views::Message {
    workspace
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .find(|message| message.id == root)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_summary_shows_the_three_latest_distinct_repliers_newest_first() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    let everyone = [&admin, &bob, &carol, &dave];
    let general = channel_id(&admin, "general");
    sync_all(&everyone).await;
    let question = admin.send_message(&general, "lunch where?", None).unwrap();
    sync_all(&everyone).await;
    for (member, text) in [
        (&carol, "pizza"),
        (&bob, "tacos"),
        (&admin, "hmm"),
        (&bob, "tacos!"),
    ] {
        member
            .send_message(&general, text, Some(question.clone()))
            .unwrap();
        sync_all(&everyone).await;
    }

    let summary = root_in(&carol, &general, &question);
    assert_eq!(
        summary.recent_reply_authors,
        vec![bob.my_id(), admin.my_id(), carol.my_id()]
    );
    assert_eq!(summary.last_reply_author, Some(bob.my_id()));
    assert_eq!(summary.last_reply_preview.as_deref(), Some("tacos!"));

    dave.send_message(&general, "sushi", Some(question.clone()))
        .unwrap();
    sync_all(&everyone).await;
    let summary = root_in(&carol, &general, &question);
    assert_eq!(
        summary.recent_reply_authors,
        vec![dave.my_id(), bob.my_id(), admin.my_id()],
        "at most three, the oldest replier drops off"
    );
    assert_eq!(summary.last_reply_author, Some(dave.my_id()));
    assert_eq!(summary.last_reply_preview.as_deref(), Some("sushi"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reply_preview_is_one_short_line_with_mentions_as_names() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "status?", None).unwrap();
    let mention = mentions::member_link("Ada", &admin.my_id());
    let long_text = format!("{mention}  done\n\nnext: {}", "日本語🎉".repeat(100));
    bob.send_message(&general, &long_text, Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let preview = root_in(&admin, &general, &question).last_reply_preview.unwrap();
    assert!(preview.starts_with("@Ada done next: 日本語🎉"), "{preview}");
    assert!(
        preview.chars().count() <= 100,
        "bounded: {} chars",
        preview.chars().count()
    );
    assert!(preview.ends_with('…'), "a cut preview says so");
}

#[tokio::test(flavor = "multi_thread")]
async fn deleted_replies_leave_the_summary() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let everyone = [&admin, &bob, &carol];
    let general = channel_id(&admin, "general");
    sync_all(&everyone).await;
    let question = admin.send_message(&general, "who's in?", None).unwrap();
    sync_all(&everyone).await;
    let bobs = bob.send_message(&general, "me", Some(question.clone())).unwrap();
    sync_all(&everyone).await;
    let carols = carol
        .send_message(&general, "me too", Some(question.clone()))
        .unwrap();
    sync_all(&everyone).await;

    carol.delete_message(&carols).unwrap();
    sync_all(&everyone).await;
    let summary = root_in(&admin, &general, &question);
    assert_eq!(summary.recent_reply_authors, vec![bob.my_id()]);
    assert_eq!(summary.last_reply_author, Some(bob.my_id()));
    assert_eq!(summary.last_reply_preview.as_deref(), Some("me"));

    bob.delete_message(&bobs).unwrap();
    sync_all(&everyone).await;
    let summary = root_in(&admin, &general, &question);
    assert_eq!(summary.reply_count, 0);
    assert!(summary.recent_reply_authors.is_empty());
    assert_eq!(summary.last_reply_author, None);
    assert_eq!(summary.last_reply_preview, None);
}
