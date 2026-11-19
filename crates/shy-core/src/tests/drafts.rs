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
