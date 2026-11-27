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
        device: oldest_loaded.device.clone(),
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

#[tokio::test(flavor = "multi_thread")]
async fn shutting_down_uploads_what_was_not_sent_yet() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "written just before quitting", None)
        .unwrap();
    bob.shutdown().await.unwrap();

    admin.sync().await;
    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(texts.contains(&"written just before quitting".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shut_down_workspace_lets_go_of_everything_and_reopens() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let dir = root.path().join("ada");
    let admin = crate::Workspace::create_with(
        &data_dir(&dir),
        TEST_WORKSPACE_ID.into(),
        "Acme",
        crate::Profile {
            name: "Ada",
            is_bot: false,
        },
        test_bucket_config(),
        connector(&bucket),
        crate::Networking::Online,
    )
    .await
    .unwrap();
    let inner = std::sync::Arc::downgrade(&admin.inner);
    admin.shutdown().await.unwrap();
    drop(admin);

    // The background loops stop and drop their handles soon after.
    let released = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while inner.upgrade().is_some() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(released.is_ok(), "background tasks still hold the workspace");
    crate::Workspace::open_with(&data_dir(&dir), connector(&bucket), crate::Networking::Offline)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_online_workspace_sends_from_a_thread_outside_its_runtime() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = Workspace::create_with(
        &data_dir(&root.path().join("ada")),
        TEST_WORKSPACE_ID.into(),
        "Acme",
        crate::Profile {
            name: "Ada",
            is_bot: false,
        },
        test_bucket_config(),
        connector(&bucket),
        Networking::Online,
    )
    .await
    .unwrap();
    let general = channel_id(&admin, "general");

    // As a window's synchronous command does, on the app's main thread.
    let sender = admin.clone();
    let channel = general.clone();
    let sent = std::thread::spawn(move || sender.send_message(&channel, "sent from the main thread", None))
        .join()
        .expect("sending outside the runtime must not panic");

    sent.unwrap();
    assert_eq!(texts_in(&admin, &general), ["sent from the main thread"]);
    admin.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn many_large_pastes_made_offline_all_reach_others() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    // Each near the page update limit; together well past the bucket's
    // object limit, so they can't go up as one segment.
    let paste = "a long chapter. ".repeat(crate::ops::MAX_PAGE_UPDATE_BYTES / 17);
    let pages: Vec<_> = (0..9)
        .map(|index| {
            let page = admin.create_page(&format!("Book {index}"), None).unwrap();
            admin.edit_page(&page, Edit::Append(&paste)).unwrap();
            page
        })
        .collect();

    sync_all(&[&admin, &bob]).await;

    for page in pages {
        assert_eq!(bob.page_text(&page).unwrap().len(), paste.len());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_details_reach_everyone_and_newcomers() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let details = crate::profile::ProfileDetails {
        title: "Engineer".into(),
        pronouns: "he/him".into(),
        time_zone: "Europe/Berlin".into(),
        working_hours: Some(crate::profile::WorkingHours {
            days: 0b001_1111,
            start_minute: 9 * 60,
            end_minute: 17 * 60,
        }),
        ..Default::default()
    };
    bob.set_profile_details(&details).unwrap();
    sync_all(&[&bob, &admin]).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;

    for member in [&admin, &bob, &carol] {
        let seen = member
            .members()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == bob.my_id())
            .unwrap()
            .details;
        assert_eq!(seen, details);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_op_that_fails_locally_is_kept_and_applied_on_the_next_sync() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "the last thing Bob wrote", None)
        .unwrap();
    bob.sync().await;

    admin
        .inner
        .fail_next_apply
        .store(true, std::sync::atomic::Ordering::SeqCst);
    admin.sync().await;
    assert!(admin.status().error.is_some(), "a local failure must show");
    let texts = |member: &Workspace| -> Vec<String> {
        member
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .map(|message| message.text)
            .collect()
    };
    assert!(!texts(&admin).contains(&"the last thing Bob wrote".to_string()));

    admin.sync().await;
    assert_eq!(admin.status().error, None);
    assert!(texts(&admin).contains(&"the last thing Bob wrote".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_op_that_cant_even_be_kept_is_read_again_next_time() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "written while the disk was full", None)
        .unwrap();
    bob.sync().await;

    for failpoint in [&admin.inner.fail_next_apply, &admin.inner.fail_next_park] {
        failpoint.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    admin.sync().await;
    assert!(admin.status().error.is_some(), "the failure must show");

    admin.sync().await;
    assert_eq!(admin.status().error, None);
    let texts: Vec<String> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(texts.contains(&"written while the disk was full".to_string()));
}

/// A downloaded segment applies in one transaction, but each op on its own:
/// one that breaks a rule is skipped and the others around it still apply.
#[tokio::test(flavor = "multi_thread")]
async fn a_broken_op_in_a_downloaded_segment_skips_only_itself() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let admins = admin.send_message(&general, "Ada's own words", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let mut forger = Forger::new(&bob);
    let message = |text: &str| crate::ops::Op::Message {
        id: crate::ops::new_owned_id(&bob.me()),
        channel: general.clone(),
        text: text.into(),
        reply_to: None,
    };
    let segment = [
        forger.seal(message("before")),
        forger.seal(crate::ops::Op::MessageEdit {
            id: admins,
            text: "not Bob's to edit".into(),
        }),
        forger.seal(message("after")),
    ];
    bucket.put_segment(&segment).await.unwrap();

    admin.sync().await;

    let texts = texts_in(&admin, &general);
    assert!(texts.contains(&"before".to_string()) && texts.contains(&"after".to_string()));
    assert!(texts.contains(&"Ada's own words".to_string()));
    assert_eq!(admin.status().error, None);
}

/// If applying a segment stops before it is saved (the app dies, say),
/// nothing of it stays and the next pull reads it again whole.
#[tokio::test(flavor = "multi_thread")]
async fn a_segment_cut_short_is_read_again_whole() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut forger = Forger::new(&bob);
    let segment = ["one", "two"].map(|text| {
        forger.seal(crate::ops::Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: text.into(),
            reply_to: None,
        })
    });
    bucket.put_segment(&segment).await.unwrap();

    admin
        .inner
        .fail_segment_save
        .store(true, std::sync::atomic::Ordering::SeqCst);
    admin.sync().await;
    assert!(admin.status().error.is_some());
    assert!(
        !texts_in(&admin, &general).contains(&"one".to_string()),
        "nothing of it stayed"
    );

    admin.sync().await;
    let texts = texts_in(&admin, &general);
    assert!(texts.contains(&"one".to_string()) && texts.contains(&"two".to_string()));
    assert_eq!(admin.status().error, None);
}

/// A bucket whose listings under `ops/` wait while the gate is closed, as
/// a long catch-up would.
#[derive(Debug)]
struct GatedStore {
    inner: InMemory,
    gate: tokio::sync::watch::Sender<bool>,
}

impl std::fmt::Display for GatedStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("gated test store")
    }
}

#[async_trait::async_trait]
impl object_store::ObjectStore for GatedStore {
    async fn put_opts(
        &self,
        location: &object_store::path::Path,
        payload: object_store::PutPayload,
        opts: object_store::PutOptions,
    ) -> object_store::Result<object_store::PutResult> {
        self.inner.put_opts(location, payload, opts).await
    }

    async fn put_multipart_opts(
        &self,
        location: &object_store::path::Path,
        opts: object_store::PutMultipartOptions,
    ) -> object_store::Result<Box<dyn object_store::MultipartUpload>> {
        self.inner.put_multipart_opts(location, opts).await
    }

    async fn get_opts(
        &self,
        location: &object_store::path::Path,
        options: object_store::GetOptions,
    ) -> object_store::Result<object_store::GetResult> {
        self.inner.get_opts(location, options).await
    }

    fn delete_stream(
        &self,
        locations: futures_lite::stream::Boxed<object_store::Result<object_store::path::Path>>,
    ) -> futures_lite::stream::Boxed<object_store::Result<object_store::path::Path>> {
        self.inner.delete_stream(locations)
    }

    fn list(
        &self,
        prefix: Option<&object_store::path::Path>,
    ) -> futures_lite::stream::Boxed<object_store::Result<object_store::ObjectMeta>> {
        use futures_lite::StreamExt;
        let listed = self.inner.list(prefix);
        if !prefix.is_some_and(|prefix| prefix.as_ref().starts_with("ops/")) {
            return listed;
        }
        let mut gate = self.gate.subscribe();
        let mut listed = Some(listed);
        Box::pin(
            futures_lite::stream::once_future(async move {
                let _ = gate.wait_for(|is_closed| !is_closed).await;
            })
            .flat_map(move |()| listed.take().expect("listed once")),
        )
    }

    async fn list_with_delimiter(
        &self,
        prefix: Option<&object_store::path::Path>,
    ) -> object_store::Result<object_store::ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        from: &object_store::path::Path,
        to: &object_store::path::Path,
        options: object_store::CopyOptions,
    ) -> object_store::Result<()> {
        self.inner.copy_opts(from, to, options).await
    }
}

/// While a long catch-up holds the sync pass, what we write still goes up
/// to the bucket.
#[tokio::test(flavor = "multi_thread")]
async fn writes_go_up_while_a_catch_up_is_still_reading() {
    use futures_lite::StreamExt;
    use object_store::ObjectStore;
    let store = Arc::new(GatedStore {
        inner: InMemory::new(),
        gate: tokio::sync::watch::Sender::new(false),
    });
    let bucket = Bucket::without_signing(store.clone());
    let root = tempfile::tempdir().unwrap();
    let admin = Workspace::create_with(
        &data_dir(&root.path().join("ada")),
        TEST_WORKSPACE_ID.into(),
        "Acme",
        crate::Profile {
            name: "Ada",
            is_bot: false,
        },
        test_bucket_config(),
        connector(&bucket),
        Networking::Online,
    )
    .await
    .unwrap();
    // Only the admin's own segments: the storage protection check puts a
    // probe beside them and deletes it again.
    let own_segments = object_store::path::Path::from(format!("ops/{}", admin.my_id()));
    let segments = || async { store.inner.list(Some(&own_segments)).count().await };
    let wait_for_more_than = |count: usize| async move {
        for _ in 0..200 {
            if segments().await > count {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        false
    };
    assert!(wait_for_more_than(0).await, "the first ops went up");
    let before = segments().await;

    store.gate.send_replace(true);
    let catching_up = tokio::spawn({
        let admin = admin.clone();
        async move { admin.sync().await }
    });
    let mut waited = 0;
    while store.gate.receiver_count() == 0 {
        assert!(waited < 1000, "the catch-up never started reading");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        waited += 1;
    }
    let general = channel_id(&admin, "general");
    admin
        .send_message(&general, "written during a catch-up", None)
        .unwrap();
    let went_up = wait_for_more_than(before).await;
    store.gate.send_replace(false);
    catching_up.await.unwrap();

    assert!(went_up, "the message went up while the pull was still reading");
    admin.shutdown().await.unwrap();
}

/// An op that writes before it fails a rule (a damaged page update first
/// adds its page) leaves nothing behind in a downloaded segment either.
#[tokio::test(flavor = "multi_thread")]
async fn a_rule_broken_halfway_leaves_nothing_in_a_downloaded_segment() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut forger = Forger::new(&bob);
    let message = |text: &str| crate::ops::Op::Message {
        id: crate::ops::new_owned_id(&bob.me()),
        channel: general.clone(),
        text: text.into(),
        reply_to: None,
    };
    let page = crate::ops::new_owned_id(&bob.me());
    let segment = [
        forger.seal(message("before")),
        forger.seal(crate::ops::Op::DocUpdate {
            page: page.clone(),
            nonce: NONCE,
            update: b"not a page update".to_vec(),
        }),
        forger.seal(message("after")),
    ];
    bucket.put_segment(&segment).await.unwrap();

    admin.sync().await;

    assert_eq!(
        admin.state().store.count_where("pages", "id", &page),
        0,
        "no page row stayed"
    );
    let texts = texts_in(&admin, &general);
    assert!(texts.contains(&"before".to_string()) && texts.contains(&"after".to_string()));
}

/// A long segment is saved a batch at a time, letting go of the state
/// between batches. If its last batch fails, the batches already saved stay,
/// and reading the segment again adds only the rest.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_segment_is_saved_in_batches_and_finished_when_read_again() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut forger = Forger::new(&bob);
    let segment: Vec<_> = (0..1_200)
        .map(|number| {
            forger.seal(crate::ops::Op::Message {
                id: crate::ops::new_owned_id(&bob.me()),
                channel: general.clone(),
                text: format!("number {number}"),
                reply_to: None,
            })
        })
        .collect();
    bucket.put_segment(&segment).await.unwrap();
    let applied = |admin: &Workspace| admin.state().store.next_seq(&bob.me()).unwrap();
    let before = applied(&admin);

    admin
        .inner
        .fail_segment_save
        .store(true, std::sync::atomic::Ordering::SeqCst);
    admin.sync().await;
    assert_eq!(applied(&admin), before + 1_000, "the two full batches stayed");

    admin.sync().await;
    assert_eq!(applied(&admin), before + 1_200);
    assert_eq!(admin.status().error, None);
}

/// If SQLite gives up a segment's transaction while an op applies (a full
/// disk can do that), nothing after it is saved as if the transaction were
/// still there, and the next pull applies the segment.
#[tokio::test(flavor = "multi_thread")]
async fn a_segment_whose_transaction_sqlite_gave_up_is_read_again_whole() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut forger = Forger::new(&bob);
    let segment: Vec<_> = (0..3)
        .map(|number| {
            forger.seal(crate::ops::Op::Message {
                id: crate::ops::new_owned_id(&bob.me()),
                channel: general.clone(),
                text: format!("number {number}"),
                reply_to: None,
            })
        })
        .collect();
    bucket.put_segment(&segment).await.unwrap();
    let applied = |admin: &Workspace| admin.state().store.next_seq(&bob.me()).unwrap();
    let applied_before = applied(&admin);

    admin
        .inner
        .drop_segment_transaction
        .store(true, std::sync::atomic::Ordering::SeqCst);
    admin.sync().await;
    assert_eq!(applied(&admin), applied_before);

    admin.sync().await;
    assert_eq!(applied(&admin), applied_before + 3);
    assert_eq!(admin.status().error, None);
}

async fn head_version(bucket: &Bucket, author: &iroh::PublicKey) -> String {
    let heads = bucket.heads().await.unwrap();
    heads
        .into_iter()
        .find(|head| head.author == *author)
        .unwrap()
        .version
}

fn texts(member: &Workspace, channel: &str) -> Vec<String> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect()
}

/// Gossip can drop the tail of a burst and its hint. The author's head then
/// moved without a hint explaining it, so the next check reads their log.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_brings_what_gossip_missed_when_a_head_moves() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    for text in ["one", "two", "three"] {
        bob.send_message(&general, text, None).unwrap();
    }
    let unpushed = bob.state().store.unpushed(&bob.me()).unwrap();
    admin.receive(unpushed[0].clone()).unwrap();
    bob.sync().await;

    assert!(admin.check().await, "the check brought ops gossip had missed");
    assert!(texts(&admin, &general).contains(&"three".to_string()));
    assert!(!admin.check().await, "nothing new the second time");
}

/// A head that a signed hint already explained (we hold up to its seq) isn't
/// read: that's what keeps checks cheap while gossip works.
#[tokio::test(flavor = "multi_thread")]
async fn a_head_a_hint_already_explained_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;
    let general = channel_id(&admin, "general");
    let mut gets_per_check = Vec::new();
    for is_hinted in [true, false] {
        bob.send_message(&general, "live", None).unwrap();
        for envelope in bob.state().store.unpushed(&bob.me()).unwrap() {
            admin.receive(envelope).unwrap();
        }
        bob.sync().await;
        if is_hinted {
            let seq = bob.state().store.last_seq(&bob.me()).unwrap();
            let hint = bob
                .signed_hint(seq, head_version(&bucket, &bob.me()).await)
                .unwrap();
            assert!(!admin.receive_hint(&hint).unwrap(), "we already hold it");
        }
        let before = store.requests();
        admin.check().await;
        gets_per_check.push(store.requests().since(&before).gets);
    }

    assert_eq!(
        gets_per_check[1],
        gets_per_check[0] + 1,
        "only the unexplained head is read"
    );
}

/// A crash between uploading a segment and writing the head leaves the head
/// behind. The author's next check rewrites it; until then the hourly sweep
/// still finds the segment.
#[tokio::test(flavor = "multi_thread")]
async fn a_segment_uploaded_without_its_head_still_arrives() {
    for is_author_back in [true, false] {
        let (_root, bucket, admin, bob) = admin_and_bob().await;
        let general = channel_id(&admin, "general");
        bob.send_message(&general, "before the crash", None).unwrap();
        let unpushed = bob.state().store.unpushed(&bob.me()).unwrap();
        bucket.put_segment(&unpushed).await.unwrap();
        let last = unpushed.last().unwrap().seq;
        bob.state().store.mark_pushed(&bob.me(), last).unwrap();
        assert!(!admin.check().await, "the head didn't move");

        if is_author_back {
            bob.check().await;
            admin.check().await;
        } else {
            admin.sync().await;
        }

        assert!(
            texts(&admin, &general).contains(&"before the crash".to_string()),
            "author back: {is_author_back}"
        );
    }
}

/// A write whose upload failed (or never started) goes up at the next check,
/// even if its author writes nothing more.
#[tokio::test(flavor = "multi_thread")]
async fn a_write_not_yet_uploaded_goes_up_at_the_next_check() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "late", None).unwrap();

    bob.check().await;
    admin.check().await;

    assert!(texts(&admin, &general).contains(&"late".to_string()));
}

/// A check that failed to read a moved head (a failed save, a network blip)
/// tries again at the next check, rather than waiting for the hourly sweep.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_that_failed_to_read_a_moved_head_tries_again() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "gossip lost this", None).unwrap();
    bob.sync().await;
    admin
        .inner
        .fail_segment_save
        .store(true, std::sync::atomic::Ordering::SeqCst);
    admin.check().await;
    assert!(admin.status().error.is_some(), "the first check failed");

    admin.check().await;

    assert!(texts(&admin, &general).contains(&"gossip lost this".to_string()));
}

/// A parked op pushed out by the cap is fetched again by the next check,
/// not only by the hourly sweep.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_repairs_a_gap() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "second", None).unwrap();
    bob.send_message(&general, "third", None).unwrap();
    bob.sync().await;
    admin
        .inner
        .fail_next_apply
        .store(true, std::sync::atomic::Ordering::SeqCst);
    admin.check().await;
    // As if the parked ops were pushed out by the cap.
    admin.state().store.execute_for_test("DELETE FROM pending_ops");
    bob.send_message(&general, "fourth", None).unwrap();
    let fourth = bob
        .state()
        .store
        .unpushed(&bob.me())
        .unwrap()
        .last()
        .unwrap()
        .clone();
    admin.receive(fourth).unwrap();

    admin.check().await;
    admin.check().await;

    let texts = texts(&admin, &general);
    for text in ["second", "third", "fourth"] {
        assert!(texts.contains(&text.to_string()), "{text}: {texts:?}");
    }
}

/// A removed member's last segment without a head (a crash before the head)
/// still reaches others at a check, up to the frozen end of their log.
#[tokio::test(flavor = "multi_thread")]
async fn a_removed_members_headless_segment_arrives_at_a_check() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "last words", None).unwrap();
    let unpushed = bob.state().store.unpushed(&bob.me()).unwrap();
    bucket.put_segment(&unpushed).await.unwrap();
    bob.state()
        .store
        .mark_pushed(&bob.me(), unpushed.last().unwrap().seq)
        .unwrap();
    admin.remove_member(&bob.my_id()).await.unwrap();

    carol.check().await;
    carol.check().await;

    assert!(texts(&carol, &general).contains(&"last words".to_string()));
}

/// Commits are checked by one listing every 10 minutes (and at sweeps);
/// only a commit whose version changed is fetched again, and one that was
/// deleted or altered is put back. A removed member who deletes the removal
/// commit can't keep returning members in the old epoch for long.
#[tokio::test(flavor = "multi_thread")]
async fn commits_are_checked_by_listing_and_put_back_when_changed() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    // A commit Bob followed (he joined after the one that added him).
    let epochs: Vec<u64> = bob
        .state()
        .store
        .commit_records()
        .unwrap()
        .into_iter()
        .map(|(epoch, _)| epoch)
        .collect();
    assert!(!epochs.is_empty());

    let slow_check = || async {
        bob.make_slow_duties_due();
        let before = store.requests();
        bob.check().await;
        store.requests().since(&before).gets
    };
    let plain_check = || async {
        let before = store.requests();
        bob.check().await;
        store.requests().since(&before).gets
    };
    slow_check().await;
    let gets_when_verified = slow_check().await;
    assert_eq!(
        gets_when_verified,
        plain_check().await,
        "unchanged commits aren't fetched"
    );

    bucket.delete_commit(epochs[0]).await.unwrap();
    slow_check().await;
    assert!(bucket.commit(epochs[0]).await.unwrap().is_some(), "put back");

    slow_check().await;
    assert_eq!(
        slow_check().await,
        gets_when_verified,
        "verified commits aren't fetched again"
    );
}

/// An idle check costs the same however many members there are: heads are
/// one listing, not one per member.
#[tokio::test(flavor = "multi_thread")]
async fn an_idle_check_costs_the_same_however_many_members() {
    let mut idle_costs = Vec::new();
    for member_count in [2, 6] {
        let root = tempfile::tempdir().unwrap();
        let (store, bucket) = super::capacity::counting_store::counting_bucket();
        let admin = create_admin(&root.path().join("ada"), &bucket).await;
        let mut members = Vec::new();
        for number in 0..member_count {
            let name = format!("member{number}");
            members.push(join(&admin, &root.path().join(&name), &bucket, &name, false).await);
        }
        let everyone: Vec<&Workspace> = std::iter::once(&admin).chain(members.iter()).collect();
        sync_all(&everyone).await;
        let reader = &members[0];
        reader.check().await;
        reader.check().await;

        let before = store.requests();
        reader.check().await;
        let cost = store.requests().since(&before);
        idle_costs.push((cost.list_pages(), cost.gets, cost.puts));
    }

    assert_eq!(idle_costs[0], idle_costs[1], "(list pages, gets, puts)");
    assert_eq!(
        idle_costs[0].0, 1,
        "only heads are listed between the 10-minute duties"
    );
}

/// An honest segment holding only ops we got live hashes the same as our own
/// copy, so a sweep checks it against the bucket without fetching it.
#[tokio::test(flavor = "multi_thread")]
async fn a_sweep_checks_segments_we_hold_without_fetching_them() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;
    let general = channel_id(&admin, "general");
    admin.sync().await;
    let before = store.requests();
    admin.sync().await;
    let idle_sweep = store.requests().since(&before).gets;

    for text in ["one", "two", "three"] {
        bob.send_message(&general, text, None).unwrap();
    }
    for envelope in bob.state().store.unpushed(&bob.me()).unwrap() {
        admin.receive(envelope).unwrap();
    }
    bob.sync().await;
    let before = store.requests();
    admin.sync().await;

    assert_eq!(
        store.requests().since(&before).gets,
        idle_sweep,
        "no segment fetched"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_hits_say_who_wrote_them_and_when() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let before = crate::ops::now_ms().unwrap();
    bob.send_message(&general, "the quarterly numbers are in", None)
        .unwrap();
    let page = admin.create_page("Quarterly plan", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let after = crate::ops::now_ms().unwrap();

    for query in ["quarterly", "from:bob"] {
        let hits = admin.search(query, 10).unwrap();
        let message = hits.iter().find(|hit| hit.kind == "message").unwrap();
        assert_eq!(message.author.as_deref(), Some(bob.my_id().as_str()), "{query}");
        assert!((before..=after).contains(&message.time_ms), "{query}");
    }
    let hits = admin.search("quarterly", 10).unwrap();
    let page_hit = hits.iter().find(|hit| hit.kind == "page").unwrap();
    assert_eq!(page_hit.id, page);
    assert_eq!(page_hit.author, None);
    let edited = admin
        .pages()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == page)
        .unwrap();
    assert_eq!(page_hit.time_ms, edited.updated_ms);
}

#[tokio::test]
async fn search_matches_only_the_text_people_see() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let plan = admin.create_page("Plan", None).unwrap();
    let markdown = format!(
        "**Budget** for [@Bob](shy://member/{bob_id}): see shy://page/{plan} and [the sheet](https://sheets.example/q3)",
        bob_id = bob.my_id(),
    );
    let message = bob.send_message(&general, &markdown, None).unwrap();
    let page = admin.create_page("Notes", None).unwrap();
    admin
        .edit_page(
            &page,
            Edit::Append("## Agenda\n\n- [x] ship the [release](shy://page/abc)"),
        )
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let found = |query: &str| -> Vec<String> {
        admin
            .search(query, 10)
            .unwrap()
            .into_iter()
            .map(|hit| hit.id)
            .filter(|id| *id == message || *id == page)
            .collect()
    };
    for query in ["budget", "bob", "sheet"] {
        assert_eq!(found(query), [message.as_str()], "{query}");
    }
    for query in ["agenda", "release"] {
        assert_eq!(found(query), [page.as_str()], "{query}");
    }
    for query in ["shy", "member", "sheets", "https", "abc", "x"] {
        assert_eq!(found(query), Vec::<String>::new(), "{query}");
    }
    let snippet = &admin.search("budget", 10).unwrap()[0].snippet;
    assert!(!snippet.contains("**") && !snippet.contains("]("), "{snippet}");
}

/// Each author also uploads their log again in aligned packs of 16, 256 and
/// 4096 ops, so a newcomer reads a long history in a few downloads rather
/// than one per upload, and so does a member who fell behind.
#[tokio::test(flavor = "multi_thread")]
async fn a_newcomer_reads_a_long_history_in_a_few_downloads() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let general = channel_id(&admin, "general");
    for number in 0..64 {
        bob.send_message(&general, &format!("message {number}"), None)
            .unwrap();
        bob.push().await.unwrap();
    }

    let before = store.requests();
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    carol.sync().await;
    let downloads = store.requests().since(&before).gets;

    let bob_held = carol.state().store.last_seq(&bob.me()).unwrap();
    assert_eq!(bob_held, bob.state().store.last_seq(&bob.me()).unwrap());
    assert!(downloads < 40, "{downloads} downloads for Bob's 64 uploads");

    for number in 0..64 {
        bob.send_message(&general, &format!("later {number}"), None)
            .unwrap();
        bob.push().await.unwrap();
    }
    let before = store.requests();
    carol.check().await;
    let downloads = store.requests().since(&before).gets;

    let bob_held = carol.state().store.last_seq(&bob.me()).unwrap();
    assert_eq!(bob_held, bob.state().store.last_seq(&bob.me()).unwrap());
    assert!(downloads < 16, "{downloads} downloads for Bob's next 64 uploads");
}

/// An author whose head moved and whose log has a gap is read once in a
/// check, not once for each reason.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_lists_each_log_once_whatever_the_reasons() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "second", None).unwrap();
    bob.send_message(&general, "third", None).unwrap();
    bob.sync().await;
    admin
        .inner
        .fail_next_apply
        .store(true, std::sync::atomic::Ordering::SeqCst);
    // The head's read fails, so the head still counts as moved next time.
    admin.check().await;
    admin.state().store.execute_for_test("DELETE FROM pending_ops");
    bob.send_message(&general, "fourth", None).unwrap();
    let fourth = bob
        .state()
        .store
        .unpushed(&bob.me())
        .unwrap()
        .last()
        .unwrap()
        .clone();
    admin.receive(fourth).unwrap();
    let bob_log = format!("ops/{}", crate::views::member_id(&bob.me()));
    let listed_before = store.listings_of(&bob_log);

    admin.check().await;

    assert_eq!(store.listings_of(&bob_log) - listed_before, 1);
    assert!(texts(&admin, &general).contains(&"third".to_string()));
}

/// A newcomer downloads each object of the history once across joining and
/// the passes after it: what is read is kept and then compared, not fetched
/// again.
#[tokio::test(flavor = "multi_thread")]
async fn a_newcomer_downloads_the_history_once_across_joining_and_the_first_passes() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let general = channel_id(&admin, "general");
    for number in 0..40 {
        bob.send_message(&general, &format!("message {number}"), None)
            .unwrap();
        bob.push().await.unwrap();
    }
    sync_all(&[&admin, &bob]).await;
    let fetched_before = store.fetched_keys().len();

    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    carol.sync().await;
    carol.sync().await;

    assert_eq!(
        carol.state().store.last_seq(&bob.me()).unwrap(),
        bob.state().store.last_seq(&bob.me()).unwrap()
    );
    let bob_log = format!("ops/{}/", crate::views::member_id(&bob.me()));
    let fetched = &store.fetched_keys()[fetched_before..];
    let mut downloads: std::collections::HashMap<&String, usize> = std::collections::HashMap::new();
    for key in fetched.iter().filter(|key| key.starts_with(&bob_log)) {
        *downloads.entry(key).or_default() += 1;
    }
    let repeated: Vec<_> = downloads.iter().filter(|(_, count)| **count > 1).collect();
    assert!(repeated.is_empty(), "downloaded again: {repeated:?}");
}

#[tokio::test]
async fn catching_up_on_typing_writes_each_page_once_per_batch_not_per_keystroke() {
    use yrs::{GetString, Text, Transact};

    const KEYSTROKES: usize = 100;
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let ada = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&ada, &root.path().join("bob"), &bucket, "Bob", false).await;
    let page = bob.create_page("Plan", None).unwrap();
    let session = bob.page_editor_session(&page);
    let editor = yrs::Doc::with_client_id(session.client_id);
    let content = editor.get_or_insert_text("content");
    for keystroke in 0..KEYSTROKES {
        let mut txn = editor.transact_mut();
        content.insert(&mut txn, keystroke as u32, "x");
        bob.apply_doc_update(&page, &session.nonce, txn.encode_update_v1())
            .unwrap();
    }
    let typed = content.get_string(&editor.transact());
    sync_all(&[&ada, &bob]).await;

    let carol = join(&ada, &root.path().join("carol"), &bucket, "Carol", false).await;
    let writes = carol.state().store.count_page_writes();
    carol.sync().await;
    assert_eq!(carol.page_text(&page).unwrap(), typed);
    // The welcome page and Bob's page, each named and then written once.
    let writes = writes.load(std::sync::atomic::Ordering::Relaxed);
    assert!(writes <= 4, "{writes} page writes for {KEYSTROKES} keystrokes");
}

/// When an app version changes the rules for showing ops, opening a store
/// built under the old rules rebuilds its views from the log, keeping
/// everything stored.
#[tokio::test(flavor = "multi_thread")]
async fn new_view_rules_rebuild_the_views_on_opening() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("ada");
    let bucket = memory_bucket();
    let admin = create_admin(&dir, &bucket).await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "kept in the log", None).unwrap();
    {
        let state = admin.state();
        state.store.execute_for_test("DELETE FROM messages");
        state
            .store
            .set_meta(crate::storage::VIEW_RULES_META, &[0])
            .unwrap();
    }
    drop(admin);

    let reopened = Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline)
        .await
        .unwrap();

    assert_eq!(texts_in(&reopened, &general), ["kept in the log"]);
}

/// Stores made before view rules were kept in `meta` have none: they are
/// rebuilt once, and then hold the current rules.
#[tokio::test(flavor = "multi_thread")]
async fn a_store_without_view_rules_is_rebuilt_once_and_records_them() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("ada");
    let bucket = memory_bucket();
    let admin = create_admin(&dir, &bucket).await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "kept in the log", None).unwrap();
    {
        let state = admin.state();
        state.store.execute_for_test("DELETE FROM messages");
        state
            .store
            .execute_for_test("DELETE FROM meta WHERE key = 'view_rules'");
    }
    drop(admin);

    let reopened = Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline)
        .await
        .unwrap();

    assert_eq!(texts_in(&reopened, &general), ["kept in the log"]);
    let rules = reopened
        .state()
        .store
        .meta(crate::storage::VIEW_RULES_META)
        .unwrap();
    assert_eq!(rules.as_deref(), Some([crate::storage::VIEW_RULES].as_slice()));
}

/// An upload costs one segment, one head, and one read of our own head to
/// check it: every request is paid for and waited on.
#[tokio::test(flavor = "multi_thread")]
async fn an_upload_makes_three_requests() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let general = channel_id(&admin, "general");
    bob.push().await.unwrap();
    bob.send_message(&general, "one more", None).unwrap();
    assert_ne!(
        bob.state().store.last_seq(&bob.me()).unwrap() % 16,
        0,
        "no pack is due"
    );

    let before = store.requests();
    bob.push().await.unwrap();
    let requests = store.requests().since(&before);

    assert_eq!(
        (requests.gets, requests.puts, requests.lists),
        (1, 2, 0),
        "{requests}"
    );
}

/// The admin sets the workspace's icon and default look; every member sees
/// them, and clearing them brings back the defaults.
#[tokio::test(flavor = "multi_thread")]
async fn the_admin_sets_the_workspace_icon_and_look_for_everyone() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let icon = [b"\x89PNG\r\n\x1a\n".as_slice(), b"acme"].concat();
    admin.set_workspace_icon(Some(icon.clone())).await.unwrap();
    admin.set_workspace_look(Some("dusk"), Some("teal")).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(bob.workspace_icon().await.unwrap(), Some(icon));
    assert!(bob.workspace_icon_id().unwrap().is_some());
    let look = bob.workspace_look().unwrap();
    assert_eq!(
        (look.theme.as_deref(), look.accent.as_deref()),
        (Some("dusk"), Some("teal"))
    );

    admin.set_workspace_icon(None).await.unwrap();
    admin.set_workspace_look(None, None).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(bob.workspace_icon().await.unwrap(), None);
    assert_eq!(
        bob.workspace_look().unwrap(),
        crate::views::WorkspaceLook::default()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_device_reads_what_was_written_before_it_joined() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "written before the phone", None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    phone.sync().await;

    assert_eq!(phone.status().error, None);
    assert!(texts_in(&phone, &general).contains(&"written before the phone".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn every_member_takes_a_linked_device_for_its_persons() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &phone, &carol]).await;

    assert_eq!(phone.my_person(), bob.me());
    let admins_epoch = admin.state().mls.epoch().unwrap();
    for member in [&admin, &bob, &phone, &carol] {
        let state = member.state();
        assert_eq!(state.store.person_of(&phone.me()).unwrap(), bob.me());
        assert_eq!(state.mls.epoch().unwrap(), admins_epoch);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_person_removes_all_their_devices() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let epoch = admin.state().mls.epoch().unwrap();

    admin.remove_member(&bob.my_id()).await.unwrap();
    bob.sync().await;
    phone.sync().await;

    assert_eq!(admin.state().mls.epoch().unwrap(), epoch + 1, "one commit");
    assert!(bob.status().removed);
    assert!(phone.status().removed);
    {
        let store = &admin.state().store;
        assert!(store.frozen_last_seq(&bob.me()).unwrap().is_some());
        assert!(store.frozen_last_seq(&phone.me()).unwrap().is_some());
    }
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "after Bob left", None).unwrap();
    admin.sync().await;
    phone.sync().await;
    assert!(!texts_in(&phone, &general).contains(&"after Bob left".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_one_device_leaves_its_siblings_working() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;

    admin.remove_devices(&[phone.my_device_id()]).await.unwrap();
    sync_all(&[&admin, &bob]).await;
    phone.sync().await;

    assert!(phone.status().removed);
    assert!(!bob.status().removed);
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "still here", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(texts_in(&admin, &general).contains(&"still here".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_device_writes_and_everyone_sees_it_under_the_persons_name() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;

    phone.send_message(&general, "from the phone", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    assert_eq!(phone.my_id(), bob.my_id());
    for member in [&admin, &bob, &phone] {
        let shown = member
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .find(|message| message.text == "from the phone")
            .unwrap();
        assert_eq!(shown.author, bob.my_id());
        assert_eq!(shown.device, phone.my_device_id());
        assert_eq!(shown.author_name, "Bob");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn my_other_devices_messages_count_as_mine_in_sent() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    bob.send_message(&general, "from the laptop", None).unwrap();
    phone.send_message(&general, "from the phone", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    for device in [&bob, &phone] {
        let mut texts: Vec<_> = device
            .sent_messages("", None, 50)
            .unwrap()
            .into_iter()
            .map(|sent| sent.message.text)
            .collect();
        texts.sort();
        assert_eq!(texts, ["from the laptop", "from the phone"]);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn paging_does_not_skip_messages_two_devices_wrote_at_the_same_clock() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    for (device, text) in [(&bob, "laptop"), (&phone, "phone")] {
        let mut forger = Forger::new(device);
        admin
            .receive(forger.seal_at(
                50_000,
                crate::ops::Op::Message {
                    id: crate::ops::new_owned_id(&device.me()),
                    channel: general.clone(),
                    text: text.into(),
                    reply_to: None,
                },
            ))
            .unwrap();
    }

    let mut seen = Vec::new();
    let mut before = None;
    for _ in 0..10 {
        let page = admin.messages(&general, before.as_ref(), 1).unwrap();
        let Some(oldest) = page.first() else {
            break;
        };
        seen.push(oldest.text.clone());
        before = Some(crate::views::MessageCursor {
            lamport: oldest.lamport,
            device: oldest.device.clone(),
        });
    }

    assert!(seen.contains(&"laptop".to_string()), "{seen:?}");
    assert!(seen.contains(&"phone".to_string()), "{seen:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn search_finds_a_linked_devices_messages_under_its_person() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    phone
        .send_message(&general, "budget from the phone", None)
        .unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let found = |member: &Workspace, query: &str| {
        member
            .search(query, 10)
            .unwrap()
            .into_iter()
            .map(|hit| hit.author)
            .collect::<Vec<_>>()
    };

    assert_eq!(found(&admin, "budget"), [Some(bob.my_id())]);
    assert_eq!(found(&admin, "from:Bob budget"), [Some(bob.my_id())]);
    // Filters with no words take the other search form.
    assert!(found(&admin, "from:Bob").contains(&Some(bob.my_id())));
    // The phone's own key isn't Bob's, so this is by person.
    assert_eq!(found(&phone, "from:me budget"), [Some(bob.my_id())]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reaction_from_my_phone_shows_as_mine_on_my_laptop() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "lunch?", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    phone.react(&message, "👍", true).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    let shown = bob
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|shown| shown.id == message)
        .unwrap();
    assert!(
        shown
            .reactions
            .iter()
            .any(|reaction| reaction.emoji == "👍" && reaction.mine)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn two_devices_replying_at_the_same_clock_leave_one_unread_after_reading_one() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "who's in?", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    for device in [&bob, &phone] {
        let reply = Forger::new(device).seal_at(
            60_000,
            crate::ops::Op::Message {
                id: crate::ops::new_owned_id(&device.me()),
                channel: general.clone(),
                text: "me".into(),
                reply_to: Some(question.clone()),
            },
        );
        admin.receive(reply).unwrap();
    }
    let replies = admin.thread(&question).unwrap();
    assert_eq!(replies.len(), 3);

    admin.mark_thread_read(&question, &replies[1].id).unwrap();

    let root = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|shown| shown.id == question)
        .unwrap();
    assert_eq!(root.unread_replies, 1);
    assert_eq!(admin.inbox().unwrap().attention.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_thread_i_started_on_my_laptop_is_followed_on_my_phone_without_my_own_replies() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    let question = bob.send_message(&general, "who's in?", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    phone
        .send_message(&general, "I am", Some(question.clone()))
        .unwrap();
    // After the phone's reply, so it comes later on every device.
    sync_all(&[&admin, &bob, &phone]).await;
    admin
        .send_message(&general, "me too", Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    for device in [&bob, &phone] {
        let root = device
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .find(|shown| shown.id == question)
            .unwrap();
        assert!(root.is_following);
        assert_eq!(root.unread_replies, 1, "only the admin's reply");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn paging_by_twos_keeps_two_devices_messages_at_one_clock_in_order() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    for (device, text) in [(&bob, "laptop"), (&phone, "phone"), (&admin, "admin")] {
        admin
            .receive(Forger::new(device).seal_at(
                70_000,
                crate::ops::Op::Message {
                    id: crate::ops::new_owned_id(&device.me()),
                    channel: general.clone(),
                    text: text.into(),
                    reply_to: None,
                },
            ))
            .unwrap();
    }

    let all = admin.messages(&general, None, 50).unwrap();
    let mut paged = Vec::new();
    let mut before = None;
    for _ in 0..10 {
        let page = admin.messages(&general, before.as_ref(), 2).unwrap();
        let Some(oldest) = page.first() else {
            break;
        };
        before = Some(crate::views::MessageCursor {
            lamport: oldest.lamport,
            device: oldest.device.clone(),
        });
        paged.splice(0..0, page.into_iter().map(|message| message.id));
    }

    let expected: Vec<_> = all.into_iter().map(|message| message.id).collect();
    assert_eq!(paged, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_shows_once_in_the_member_list_with_their_newest_name() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;

    phone
        .set_profile(crate::Profile {
            name: "Bobby",
            is_bot: false,
        })
        .unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    for member in [&admin, &bob, &phone] {
        let members = member.members().unwrap();
        let bobs: Vec<_> = members.iter().filter(|shown| shown.id == bob.my_id()).collect();
        assert_eq!(bobs.len(), 1);
        assert_eq!(bobs[0].name, "Bobby");
        assert!(members.iter().all(|shown| shown.id != phone.my_device_id()));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reaction_made_on_one_device_can_be_taken_back_on_another() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "lunch?", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let thumbs = |member: &Workspace| {
        member
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .find(|shown| shown.id == message)
            .unwrap()
            .reactions
            .into_iter()
            .find(|reaction| reaction.emoji == "👍")
    };

    bob.react(&message, "👍", true).unwrap();
    phone.react(&message, "👍", true).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    for member in [&admin, &bob, &phone] {
        assert_eq!(thumbs(member).unwrap().count, 1, "one person, one reaction");
    }
    assert!(thumbs(&phone).unwrap().mine);

    phone.react(&message, "👍", false).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    for member in [&admin, &bob, &phone] {
        assert!(thumbs(member).is_none());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn details_a_picture_and_a_name_set_on_my_phone_show_for_me() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    let picture = [b"\x89PNG\r\n\x1a\n".as_slice(), b"phone"].concat();

    phone
        .set_profile(crate::Profile {
            name: "Phone Bob",
            is_bot: false,
        })
        .unwrap();
    phone
        .set_profile_details(&crate::profile::ProfileDetails {
            title: "Engineer".into(),
            ..Default::default()
        })
        .unwrap();
    phone.set_profile_picture(Some(picture.clone())).await.unwrap();
    phone.send_message(&general, "from the phone", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    let shown = admin
        .members()
        .unwrap()
        .into_iter()
        .find(|member| member.id == bob.my_id())
        .unwrap();
    assert_eq!(shown.details.title, "Engineer");
    assert!(shown.picture.is_some());
    assert_eq!(admin.profile_picture(&bob.me()).await.unwrap(), Some(picture));
    let message = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|message| message.text == "from the phone")
        .unwrap();
    assert_eq!(message.author_name, "Phone Bob");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_stays_active_while_any_device_remains_even_after_losing_the_first() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &phone]).await;
    let is_active = |member: &Workspace| {
        member
            .members()
            .unwrap()
            .into_iter()
            .find(|shown| shown.id == bob.my_id())
            .unwrap()
            .active
    };

    admin.remove_devices(&[bob.my_device_id()]).await.unwrap();
    sync_all(&[&admin, &phone]).await;
    phone.send_message(&general, "still here", None).unwrap();
    sync_all(&[&admin, &phone]).await;

    assert!(is_active(&admin));
    let message = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|message| message.text == "still here")
        .unwrap();
    assert_eq!(message.author, bob.my_id());
    assert_eq!(message.author_name, "Bob");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reaction_another_of_my_devices_took_back_frees_room_on_this_one() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "news", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let faces: Vec<String> = ('\u{1F600}'..='\u{1F63F}').map(String::from).collect();
    let cap = crate::ops::MAX_REACTIONS_PER_AUTHOR;

    for face in &faces[..cap] {
        phone.react(&message, face, true).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;
    for face in &faces[..cap] {
        bob.react(&message, face, false).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;

    phone.react(&message, &faces[cap], true).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    for member in [&admin, &bob, &phone] {
        let shown: Vec<String> = member
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .find(|shown| shown.id == message)
            .unwrap()
            .reactions
            .into_iter()
            .map(|reaction| reaction.emoji)
            .collect();
        assert_eq!(shown, [faces[cap].clone()]);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn after_removing_a_device_the_remaining_one_overrides_what_it_set() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "news", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let picture = |text: &[u8]| [b"\x89PNG\r\n\x1a\n".as_slice(), text].concat();
    let status = |text: &'static str| crate::views::StatusUpdate {
        emoji: "🎧",
        text,
        until_ms: None,
        alerts: crate::views::NotifyLevel::Nothing,
    };
    // A stolen phone sets everything a person shows, then is removed.
    phone
        .set_profile(crate::Profile {
            name: "Thief",
            is_bot: false,
        })
        .unwrap();
    phone
        .set_profile_details(&crate::profile::ProfileDetails {
            title: "Thief".into(),
            ..Default::default()
        })
        .unwrap();
    phone.set_profile_picture(Some(picture(b"thief"))).await.unwrap();
    phone.set_status(status("Thief")).unwrap();
    phone.react(&message, "👎", true).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    admin.remove_devices(&[phone.my_device_id()]).await.unwrap();
    sync_all(&[&admin, &bob]).await;

    bob.set_profile(crate::Profile {
        name: "Bob",
        is_bot: false,
    })
    .unwrap();
    bob.set_profile_details(&crate::profile::ProfileDetails {
        title: "Engineer".into(),
        ..Default::default()
    })
    .unwrap();
    bob.set_profile_picture(Some(picture(b"bob"))).await.unwrap();
    bob.set_status(status("Back")).unwrap();
    bob.react(&message, "👎", false).unwrap();
    sync_all(&[&admin, &bob]).await;

    let shown = admin
        .members()
        .unwrap()
        .into_iter()
        .find(|member| member.id == bob.my_id())
        .unwrap();
    assert_eq!(shown.name, "Bob");
    assert_eq!(shown.details.title, "Engineer");
    assert_eq!(shown.status.unwrap().text, "Back");
    assert_eq!(
        admin.profile_picture(&bob.me()).await.unwrap(),
        Some(picture(b"bob"))
    );
    let reactions = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|shown| shown.id == message)
        .unwrap()
        .reactions;
    assert!(reactions.is_empty(), "{reactions:?}");
}

fn reactions_shown(member: &Workspace, channel: &str, message: &str) -> Vec<String> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .find(|shown| shown.id == message)
        .unwrap()
        .reactions
        .into_iter()
        .map(|reaction| reaction.emoji)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn freeing_room_for_a_reaction_can_hide_only_the_one_emoji_it_frees() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "news", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let faces: Vec<String> = ('\u{1F600}'..='\u{1F63F}').map(String::from).collect();
    let cap = crate::ops::MAX_REACTIONS_PER_AUTHOR;
    for face in &faces[..cap] {
        phone.react(&message, face, true).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;
    for face in &faces[..cap] {
        bob.react(&message, face, false).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;

    // The laptop adds three again, which the phone hasn't heard of yet.
    for face in &faces[..3] {
        bob.react(&message, face, true).unwrap();
    }
    phone.react(&message, &faces[cap], true).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    // The laptop's re-add of faces[0] and the phone's take-back of it land
    // on one clock, so the device keys decide; nothing else is touched.
    let is_readd_kept = bob.me().as_bytes() > phone.me().as_bytes();
    let mut expected = vec![faces[1].clone(), faces[2].clone(), faces[cap].clone()];
    if is_readd_kept {
        expected.push(faces[0].clone());
    }
    expected.sort();
    for member in [&admin, &bob, &phone] {
        let mut shown = reactions_shown(member, &general, &message);
        shown.sort();
        assert_eq!(shown, expected);
        assert_eq!(shown.contains(&faces[0]), is_readd_kept);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_freed_below_the_reaction_limit() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "news", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let faces: Vec<String> = ('\u{1F600}'..='\u{1F63F}').map(String::from).collect();
    for face in &faces[..2] {
        phone.react(&message, face, true).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;
    for face in &faces[..2] {
        bob.react(&message, face, false).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;

    phone.react(&message, &faces[2], true).unwrap();

    let oldest = phone
        .state()
        .store
        .oldest_own_reaction_shown_off(&message, &phone.me())
        .unwrap();
    assert_eq!(oldest, Some(faces[0].clone()), "its stale rows stay as they were");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reaction_taken_back_longest_ago_is_the_one_freed() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "news", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let faces: Vec<String> = ('\u{1F600}'..='\u{1F63F}').map(String::from).collect();
    let cap = crate::ops::MAX_REACTIONS_PER_AUTHOR;
    for face in &faces[..cap] {
        phone.react(&message, face, true).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;
    // Taken back last first, so the last one added is the oldest take-back.
    for face in faces[..cap].iter().rev() {
        bob.react(&message, face, false).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;
    let oldest = |device: &Workspace| {
        device
            .state()
            .store
            .oldest_own_reaction_shown_off(&message, &device.me())
            .unwrap()
    };
    assert_eq!(oldest(&phone), Some(faces[cap - 1].clone()));

    phone.react(&message, &faces[cap], true).unwrap();

    assert_eq!(
        oldest(&phone),
        Some(faces[cap - 2].clone()),
        "only that one was freed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn my_phone_taking_back_my_laptops_reactions_frees_room_on_the_laptop() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "news", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    let faces: Vec<String> = ('\u{1F600}'..='\u{1F63F}').map(String::from).collect();
    let cap = crate::ops::MAX_REACTIONS_PER_AUTHOR;

    for face in &faces[..cap] {
        bob.react(&message, face, true).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;
    for face in &faces[..cap] {
        phone.react(&message, face, false).unwrap();
    }
    sync_all(&[&admin, &bob, &phone]).await;
    bob.react(&message, &faces[cap], true).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    for member in [&admin, &bob, &phone] {
        assert_eq!(reactions_shown(member, &general, &message), [faces[cap].clone()]);
    }
}
