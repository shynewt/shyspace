//! A notification's quick actions: a reply goes where the alerted message
//! is, an unsent one becomes a draft there, and "mark as read" reads that
//! conversation.

use super::support::*;

fn unread(workspace: &crate::Workspace, channel: &str) -> u32 {
    workspace
        .channels()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == channel)
        .unwrap()
        .unread
}

fn unread_replies(workspace: &crate::Workspace, channel: &str, root: &str) -> u32 {
    workspace
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .find(|message| message.id == root)
        .unwrap()
        .unread_replies
}

fn draft(workspace: &crate::Workspace, target: &str) -> Option<String> {
    workspace
        .drafts()
        .unwrap()
        .into_iter()
        .find(|draft| draft.target == target)
        .map(|draft| draft.text)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_to_a_channel_message_goes_in_the_channel() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = bob.send_message(&general, "lunch?", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    admin.reply_to_alert(&question, "yes").unwrap();
    sync_all(&[&admin, &bob]).await;

    let latest = bob.messages(&general, None, 1).unwrap().pop().unwrap();
    assert_eq!(latest.text, "yes");
    assert_eq!(latest.reply_to, None, "not a thread of its own");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_to_a_thread_reply_stays_in_that_thread() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "thoughts?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let answer = bob
        .send_message(&general, "looks good", Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    admin.reply_to_alert(&answer, "thanks").unwrap();

    let thread = admin.thread(&question).unwrap();
    assert_eq!(thread.last().unwrap().text, "thanks");
    assert_eq!(
        admin.messages(&general, None, 1).unwrap().pop().unwrap().id,
        question,
        "nothing new in the channel itself"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unsent_reply_is_kept_as_a_draft_after_any_already_there() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "thoughts?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let in_channel = bob.send_message(&general, "lunch?", None).unwrap();
    let in_thread = bob
        .send_message(&general, "looks good", Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    admin.set_draft(&general, "half a thought").unwrap();

    admin.keep_unsent_reply(&in_channel, "yes").unwrap();
    admin.keep_unsent_reply(&in_thread, "thanks").unwrap();

    assert_eq!(draft(&admin, &general).as_deref(), Some("half a thought\n\nyes"));
    assert_eq!(draft(&admin, &question).as_deref(), Some("thanks"));
}

#[tokio::test(flavor = "multi_thread")]
async fn mark_as_read_reads_the_whole_channel() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let first = bob.send_message(&general, "one", None).unwrap();
    bob.send_message(&general, "two", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(unread(&admin, &general), 2);

    admin.mark_alert_read(&first).unwrap();

    assert_eq!(unread(&admin, &general), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn mark_as_read_on_a_thread_reply_reads_that_thread() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "thoughts?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let first = bob
        .send_message(&general, "looks good", Some(question.clone()))
        .unwrap();
    bob.send_message(&general, "ship it", Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(unread_replies(&admin, &general, &question), 2);

    admin.mark_alert_read(&first).unwrap();

    assert_eq!(unread_replies(&admin, &general, &question), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn actions_on_a_message_that_is_gone_say_so() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;

    let error = admin.reply_to_alert("missing", "hello").unwrap_err();

    assert!(error.to_string().contains("no longer there"), "{error}");
    assert!(admin.mark_alert_read("missing").is_err());
}
