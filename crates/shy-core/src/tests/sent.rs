//! Sent: your own messages, newest first, from channels you can still read.

use super::support::*;
use crate::views::{LinkedMessage, MessageCursor};

fn texts(sent: &[LinkedMessage]) -> Vec<String> {
    sent.iter().map(|item| item.message.text.clone()).collect()
}

fn cursor_after(sent: &[LinkedMessage]) -> MessageCursor {
    let oldest = &sent.last().unwrap().message;
    MessageCursor {
        lamport: oldest.lamport,
        author: oldest.author.clone(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sent_lists_only_your_own_messages_newest_first() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let random = admin.create_channel("random").unwrap();
    let question = admin.send_message(&general, "first", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    bob.send_message(&general, "not mine", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    admin.send_message(&random, "second", None).unwrap();
    let reply = admin
        .send_message(&general, "third, in a thread", Some(question.clone()))
        .unwrap();

    let sent = admin.sent_messages("", None, 50).unwrap();

    assert_eq!(texts(&sent), ["third, in a thread", "second", "first"]);
    assert_eq!(sent[0].message.id, reply);
    assert_eq!(sent[0].thread_root.as_deref(), Some(question.as_str()));
    assert_eq!(sent[1].message.channel, random);
    assert_eq!(sent[2].thread_root, None);
    assert_eq!(texts(&bob.sent_messages("", None, 50).unwrap()), ["not mine"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn sent_messages_show_how_people_responded() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "who reviews?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    bob.send_message(&general, "I can", Some(question.clone()))
        .unwrap();
    bob.react(&question, "👍", true).unwrap();
    sync_all(&[&admin, &bob]).await;

    let sent = admin.sent_messages("", None, 50).unwrap();

    let message = &sent[0].message;
    assert_eq!(message.reply_count, 1);
    assert_eq!(message.recent_reply_authors, [bob.my_id()]);
    assert_eq!(message.reactions.len(), 1);
    assert_eq!(message.reactions[0].count, 1);
    assert!(message.can_edit, "your own messages are marked as yours to edit");
}

#[tokio::test(flavor = "multi_thread")]
async fn sent_pages_stay_stable_while_you_keep_writing() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    for number in 1..=5 {
        admin
            .send_message(&general, &format!("message {number}"), None)
            .unwrap();
    }

    let first = admin.sent_messages("", None, 2).unwrap();
    admin.send_message(&general, "written meanwhile", None).unwrap();
    let second = admin.sent_messages("", Some(&cursor_after(&first)), 2).unwrap();
    let third = admin.sent_messages("", Some(&cursor_after(&second)), 2).unwrap();

    assert_eq!(texts(&first), ["message 5", "message 4"]);
    assert_eq!(texts(&second), ["message 3", "message 2"]);
    assert_eq!(texts(&third), ["message 1"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sent_filter_matches_words_by_their_start() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin
        .send_message(&general, "the release is Friday", None)
        .unwrap();
    admin.send_message(&general, "lunch?", None).unwrap();
    let edited = admin.send_message(&general, "old words", None).unwrap();
    admin.edit_message(&edited, "release notes are up").unwrap();

    assert_eq!(
        texts(&admin.sent_messages("rele", None, 50).unwrap()),
        ["release notes are up", "the release is Friday"]
    );
    assert_eq!(
        texts(&admin.sent_messages("release fri", None, 50).unwrap()),
        ["the release is Friday"]
    );
    assert!(admin.sent_messages("old", None, 50).unwrap().is_empty());
    assert!(
        admin.sent_messages("\"", None, 50).unwrap().is_empty(),
        "quotes in the filter are plain text"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sent_leaves_out_private_channels_you_left() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let plans = bob.create_private_channel("plans", &[admin.my_id()]).unwrap();
    sync_all(&[&admin, &bob]).await;
    admin.send_message(&plans, "secret plan", None).unwrap();
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "out in the open", None).unwrap();
    assert_eq!(
        texts(&admin.sent_messages("", None, 50).unwrap()),
        ["out in the open", "secret plan"]
    );

    admin.leave_private_channel(&plans).unwrap();

    assert_eq!(
        texts(&admin.sent_messages("", None, 50).unwrap()),
        ["out in the open"]
    );
    assert_eq!(
        texts(&admin.sent_messages("secret", None, 50).unwrap()),
        Vec::<String>::new()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sent_leaves_out_deleted_messages() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "keep this", None).unwrap();
    let regret = admin.send_message(&general, "regret this", None).unwrap();

    admin.delete_message(&regret).unwrap();

    assert_eq!(texts(&admin.sent_messages("", None, 50).unwrap()), ["keep this"]);
}
