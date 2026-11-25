//! Drafts: unsent text per channel or thread, kept on this device only.

use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn drafts_are_kept_per_place_and_cleared_when_empty_or_sent() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "release?", None).unwrap();

    admin.set_draft(&general, "half a thought").unwrap();
    admin.set_draft(&question, "a reply in progress").unwrap();
    let drafts: Vec<_> = admin
        .drafts()
        .unwrap()
        .into_iter()
        .map(|d| (d.target, d.text))
        .collect();
    assert!(drafts.contains(&(general.clone(), "half a thought".to_string())));
    assert!(drafts.contains(&(question.clone(), "a reply in progress".to_string())));
    let thread_draft = admin
        .drafts()
        .unwrap()
        .into_iter()
        .find(|d| d.target == question)
        .unwrap();
    assert_eq!(thread_draft.channel, general, "a thread draft knows its channel");

    admin.set_draft(&question, "   ").unwrap();
    assert!(
        admin.drafts().unwrap().iter().all(|d| d.target != question),
        "blank drafts are dropped"
    );

    admin.send_message(&general, "the whole thought", None).unwrap();
    assert!(admin.drafts().unwrap().is_empty(), "sending clears the draft");
    sync_all(&[&admin, &bob]).await;
    assert!(bob.drafts().unwrap().is_empty(), "drafts never leave the device");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unsaved_edit_is_kept_apart_from_the_conversations_draft() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let sent = admin.send_message(&general, "lunch at noon", None).unwrap();

    admin.set_draft(&general, "a new thought").unwrap();
    admin.set_edit_draft(&general, &sent, "lunch at one").unwrap();
    let drafts = admin.drafts().unwrap();
    let draft = drafts.iter().find(|d| d.editing.is_none()).unwrap();
    assert_eq!(
        (draft.target.as_str(), draft.text.as_str()),
        (general.as_str(), "a new thought")
    );
    let edit = drafts.iter().find(|d| d.editing.is_some()).unwrap();
    assert_eq!(
        (edit.target.as_str(), edit.channel.as_str()),
        (general.as_str(), general.as_str())
    );
    assert_eq!(edit.text, "lunch at one");
    let editing = edit.editing.as_ref().unwrap();
    assert_eq!(editing.message, sent);
    assert!(!editing.is_deleted);

    admin.set_edit_draft(&general, &sent, "").unwrap();
    let drafts = admin.drafts().unwrap();
    assert_eq!(drafts.len(), 1, "a blank edit is dropped, the draft stays");
    assert!(drafts[0].editing.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_in_a_thread_is_kept_for_that_thread() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let root = admin.send_message(&general, "release?", None).unwrap();
    let reply = admin
        .send_message(&general, "friday", Some(root.clone()))
        .unwrap();

    admin.set_edit_draft(&root, &reply, "thursday").unwrap();
    let edit = admin.drafts().unwrap().pop().unwrap();
    assert_eq!((edit.target, edit.channel), (root, general));
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_are_only_kept_for_your_own_messages_where_they_are() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let random = admin.create_channel("random").unwrap();
    let mine = admin.send_message(&general, "mine", None).unwrap();
    let bobs = bob.send_message(&general, "bob's", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(admin.set_edit_draft(&general, &bobs, "not mine to edit").is_err());
    assert!(
        admin
            .set_edit_draft(&random, &mine, "in the wrong channel")
            .is_err()
    );
    assert!(
        admin
            .set_edit_draft(&general, "missing", "no such message")
            .is_err()
    );
    assert!(admin.drafts().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_of_a_deleted_message_is_kept_after_the_draft_there() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let sent = admin.send_message(&general, "lunch at noon", None).unwrap();
    admin.set_edit_draft(&general, &sent, "lunch at one").unwrap();
    assert!(
        admin.keep_edit_of_deleted_message(&general, &sent).is_err(),
        "an edit of a message still there stays an edit"
    );

    admin.delete_message(&sent).unwrap();
    let edit = admin.drafts().unwrap().pop().unwrap();
    assert!(
        edit.editing.unwrap().is_deleted,
        "the text isn't dropped with the message"
    );

    admin.set_draft(&general, "a new thought").unwrap();
    admin.keep_edit_of_deleted_message(&general, &sent).unwrap();
    let drafts = admin.drafts().unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].text, "a new thought\n\nlunch at one");
    assert!(drafts[0].editing.is_none());

    sync_all(&[&admin, &bob]).await;
    assert!(bob.drafts().unwrap().is_empty(), "edits never leave the device");
}
