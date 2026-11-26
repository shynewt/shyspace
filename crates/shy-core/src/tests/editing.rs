//! Editing and deleting: messages, channels and pages, seen by other members.

use super::support::*;

fn texts(workspace: &crate::Workspace, channel: &str) -> Vec<String> {
    workspace
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect()
}

fn search_ids(workspace: &crate::Workspace, query: &str) -> Vec<String> {
    workspace
        .search(query, 10)
        .unwrap()
        .into_iter()
        .map(|hit| hit.id)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edited_then_deleted_message_updates_everywhere() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = bob.send_message(&general, "lunch at noon", None).unwrap();
    bob.edit_message(&message, "lunch at one").unwrap();
    sync_all(&[&admin, &bob]).await;

    let seen = admin.messages(&general, None, 50).unwrap();
    let edited = seen.iter().find(|candidate| candidate.id == message).unwrap();
    assert_eq!(edited.text, "lunch at one");
    assert!(edited.is_edited);
    assert!(
        search_ids(&admin, "noon").is_empty(),
        "the old text is gone from search"
    );
    assert_eq!(search_ids(&admin, "one"), vec![message.clone()]);

    bob.delete_message(&message).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(!texts(&admin, &general).contains(&"lunch at one".to_string()));
    assert!(search_ids(&admin, "lunch").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_author_edits_and_the_admin_may_also_delete() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let admins = admin.send_message(&general, "from ada", None).unwrap();
    let bobs = bob.send_message(&general, "spam spam", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(bob.edit_message(&admins, "rewritten").is_err());
    assert!(bob.delete_message(&admins).is_err());
    assert!(
        admin.edit_message(&bobs, "rewritten").is_err(),
        "not even the admin edits others"
    );

    admin.delete_message(&bobs).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(
        texts(&bob, &general)
            .iter()
            .filter(|text| *text == "spam spam")
            .count(),
        0
    );
    assert!(texts(&bob, &general).contains(&"from ada".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn channels_can_be_renamed_by_anyone_and_deleted_by_their_owner() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let random = bob.create_channel("random").unwrap();
    bob.send_message(&random, "a cat picture", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    admin.rename_channel(&random, "Off Topic").unwrap();
    sync_all(&[&admin, &bob]).await;
    let names: Vec<_> = bob
        .channels()
        .unwrap()
        .into_iter()
        .map(|channel| channel.name)
        .collect();
    assert!(names.contains(&"off-topic".to_string()), "{names:?}");

    let general = channel_id(&admin, "general");
    assert!(bob.delete_channel(&general).is_err(), "bob doesn't own #general");
    bob.delete_channel(&random).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(
        admin
            .channels()
            .unwrap()
            .iter()
            .all(|channel| channel.id != random)
    );
    assert!(admin.messages(&random, None, 50).unwrap().is_empty());
    assert!(search_ids(&admin, "cat").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_page_moves_its_sub_pages_to_the_top() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let parent = admin.create_page("Projects", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let child = bob.create_page("Bob's notes", Some(parent.clone())).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(bob.delete_page(&parent).is_err(), "bob doesn't own the parent");
    admin.delete_page(&parent).unwrap();
    sync_all(&[&admin, &bob]).await;

    let pages = bob.pages().unwrap();
    assert!(pages.iter().all(|page| page.id != parent));
    let notes = pages.iter().find(|page| page.id == child).unwrap();
    assert_eq!(notes.parent, None);
    assert!(search_ids(&bob, "Projects").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn views_say_what_each_member_may_change() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.create_channel("random").unwrap();
    admin.send_message(&general, "from ada", None).unwrap();
    bob.send_message(&general, "from bob", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let permissions = |workspace: &crate::Workspace| -> Vec<(String, bool, bool)> {
        workspace
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .map(|message| (message.text, message.can_edit, message.can_delete))
            .collect()
    };
    assert_eq!(
        permissions(&bob),
        vec![("from ada".into(), false, false), ("from bob".into(), true, true)]
    );
    assert_eq!(
        permissions(&admin),
        vec![("from ada".into(), true, true), ("from bob".into(), false, true)]
    );
    let deletable = |workspace: &crate::Workspace| -> Vec<bool> {
        let mut channels = workspace.channels().unwrap();
        channels.sort_by_key(|channel| channel.id != general);
        channels.into_iter().map(|channel| channel.can_delete).collect()
    };
    assert_eq!(deletable(&bob), vec![false, true]);
    assert_eq!(deletable(&admin), vec![true, true]);
}

#[tokio::test(flavor = "multi_thread")]
async fn my_second_device_can_edit_and_delete_my_message() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = bob.send_message(&general, "from the laptop", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    assert!(
        phone
            .messages(&general, None, 50)
            .unwrap()
            .iter()
            .any(|shown| shown.id == message && shown.can_edit && shown.can_delete)
    );

    phone.edit_message(&message, "fixed on the phone").unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    for device in [&admin, &bob, &phone] {
        assert!(texts(device, &general).contains(&"fixed on the phone".to_string()));
    }

    phone.delete_message(&message).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    for device in [&admin, &bob, &phone] {
        assert!(!texts(device, &general).contains(&"fixed on the phone".to_string()));
    }
}
