//! Members working together: joining, chat, pages, search, removal, restart.

use super::support::*;
use std::sync::Arc;

use object_store::memory::InMemory;

use crate::{Edit, Networking, Workspace, transport::bucket::Bucket, views::Change};

#[tokio::test(flavor = "multi_thread")]
async fn two_members_collaborate_then_one_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let general = channel_id(&admin, "general");
    admin
        .send_message(&general, "hello before bob joined", None)
        .unwrap();
    admin.sync().await;

    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;

    // History from before the join is readable thanks to the keyring.
    let seen_by_bob = bob.messages(&general, None, 50).unwrap();
    assert_eq!(seen_by_bob[0].text, "hello before bob joined");
    assert_eq!(seen_by_bob[0].author_name, "Ada");
    let page = bob
        .pages()
        .unwrap()
        .into_iter()
        .find(|page| page.title == "Getting started")
        .unwrap();
    assert!(
        bob.page_text(&page.id)
            .unwrap()
            .starts_with("# Welcome to shyspace")
    );

    // Chat, replies and reactions flow both ways.
    let question = bob
        .send_message(&general, "hi Ada, quick question", None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    admin
        .send_message(&general, "sure, go ahead", Some(question.clone()))
        .unwrap();
    admin.react(&question, "👍", true).unwrap();
    bob.react(&question, "👍", true).unwrap();
    sync_all(&[&admin, &bob]).await;
    let messages = bob.messages(&general, None, 50).unwrap();
    let asked = messages.iter().find(|message| message.id == question).unwrap();
    assert_eq!(asked.reactions[0].count, 2);
    assert!(asked.reactions[0].mine);
    // The reply lives in the question's thread.
    assert_eq!(asked.reply_count, 1);
    let thread = bob.thread(&question).unwrap();
    assert_eq!(thread.last().unwrap().text, "sure, go ahead");
    assert_eq!(
        thread.last().unwrap().reply_to.as_deref(),
        Some(question.as_str())
    );
    let members = admin.members().unwrap();
    assert!(
        members
            .iter()
            .any(|member| member.name == "Bob" && member.active && !member.is_admin)
    );

    // Concurrent edits to one page converge.
    admin.edit_page(&page.id, Edit::Append("Ada was here.")).unwrap();
    bob.edit_page(
        &page.id,
        Edit::Replace {
            find: "# Welcome to shyspace",
            with: "# Welcome, team",
        },
    )
    .unwrap();
    sync_all(&[&admin, &bob]).await;
    let text = admin.page_text(&page.id).unwrap();
    assert_eq!(text, bob.page_text(&page.id).unwrap());
    assert!(text.starts_with("# Welcome, team") && text.ends_with("Ada was here."));

    // Search finds messages and pages, locally.
    let hits = bob.search("quest", 10).unwrap();
    assert!(hits.iter().any(|hit| hit.kind == "message" && hit.id == question));
    assert!(
        bob.search("team", 10)
            .unwrap()
            .iter()
            .any(|hit| hit.kind == "page")
    );

    // Removal: Bob loses access to anything written afterwards.
    let mut bob_events = bob.subscribe();
    admin.remove_member(&bob.my_id()).await.unwrap();
    admin
        .send_message(&general, "secret after removal", None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(bob.status().removed);
    assert!(bob.send_message(&general, "still here?", None).is_err());
    assert!(
        !bob.messages(&general, None, 50)
            .unwrap()
            .iter()
            .any(|message| message.text.contains("secret"))
    );
    let mut saw_removed = false;
    while let Ok(change) = bob_events.try_recv() {
        saw_removed |= change == Change::Removed;
    }
    assert!(saw_removed);
    assert!(
        !admin
            .members()
            .unwrap()
            .iter()
            .any(|member| member.name == "Bob" && member.active)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reopening_keeps_state_and_keys() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("ada");
    let bucket = memory_bucket();
    let admin = create_admin(&dir, &bucket).await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "persist me", None).unwrap();
    drop(admin);

    let reopened = Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    assert_eq!(
        reopened.messages(&general, None, 10).unwrap()[0].text,
        "persist me"
    );
    reopened.send_message(&general, "and keep writing", None).unwrap();
    assert_eq!(reopened.messages(&general, None, 10).unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn bucket_never_sees_plaintext() {
    let root = tempfile::tempdir().unwrap();
    let memory = Arc::new(InMemory::new());
    let bucket = Bucket::without_signing(memory.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "TOP-SECRET-PLAN", None).unwrap();
    admin.sync().await;

    use futures_lite::StreamExt;
    use object_store::{ObjectStore, ObjectStoreExt};
    let mut listing = memory.list(None);
    let mut objects = 0;
    while let Some(meta) = listing.next().await {
        let bytes = memory
            .get(&meta.unwrap().location)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        objects += 1;
        assert!(!bytes.windows(15).any(|window| window == b"TOP-SECRET-PLAN"));
        assert!(!bytes.windows(7).any(|window| window == b"general"));
    }
    assert!(objects > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn older_messages_load_page_by_page() {
    let root = tempfile::tempdir().unwrap();
    let admin = create_admin(&root.path().join("ada"), &memory_bucket()).await;
    let general = channel_id(&admin, "general");
    for index in 0..5 {
        admin
            .send_message(&general, &format!("message {index}"), None)
            .unwrap();
    }

    let latest = admin.messages(&general, None, 2).unwrap();
    let texts =
        |messages: &[crate::views::Message]| messages.iter().map(|m| m.text.clone()).collect::<Vec<_>>();
    assert_eq!(texts(&latest), ["message 3", "message 4"]);

    let oldest_loaded = &latest[0];
    let cursor = crate::views::MessageCursor {
        lamport: oldest_loaded.lamport,
        author: oldest_loaded.author.clone(),
    };
    let older = admin.messages(&general, Some(&cursor), 2).unwrap();
    assert_eq!(texts(&older), ["message 1", "message 2"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_data_folder_opens_in_one_place_at_a_time() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("ada");
    let bucket = memory_bucket();
    let admin = create_admin(&dir, &bucket).await;
    let second = Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline).await;
    let error = second.err().unwrap();
    assert!(format!("{error:#}").contains("already open"), "{error:#}");

    drop(admin);
    Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline)
        .await
        .unwrap();
}
