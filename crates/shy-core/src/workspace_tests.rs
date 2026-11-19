use std::{path::Path, sync::Arc, time::Duration};

use object_store::memory::InMemory;

use crate::{
    Edit, Networking, Profile, Workspace,
    bucket::{Bucket, BucketConfig},
    invite::Invite,
    views::Change,
};

fn test_bucket_config() -> BucketConfig {
    BucketConfig {
        endpoint: "http://unused".into(),
        region: "us-east-1".into(),
        bucket: "unused".into(),
        access_key_id: "unused".into(),
        secret_access_key: "unused".into(),
    }
}

async fn create_admin(dir: &Path, bucket: &Bucket) -> Workspace {
    Workspace::create_with(
        dir,
        "workspace-id".into(),
        "Acme",
        Profile {
            name: "Ada",
            is_bot: false,
        },
        test_bucket_config(),
        bucket.clone(),
        Networking::Offline,
    )
    .await
    .unwrap()
}

/// Bob asks to join while the admin's sync loop (driven here) accepts him.
async fn join(admin: &Workspace, dir: &Path, bucket: &Bucket, name: &str, is_bot: bool) -> Workspace {
    let invite = Invite::decode(&admin.invite_code().unwrap()).unwrap();
    let dir = dir.to_path_buf();
    let bucket_for_joiner = bucket.clone();
    let name = name.to_owned();
    let joining = tokio::spawn(async move {
        Workspace::join_with(
            &dir,
            invite,
            Profile { name: &name, is_bot },
            bucket_for_joiner,
            Networking::Offline,
        )
        .await
        .unwrap()
    });
    while !joining.is_finished() {
        admin.sync().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    joining.await.unwrap()
}

async fn sync_all(members: &[&Workspace]) {
    for _ in 0..2 {
        for member in members {
            member.sync().await;
            assert_eq!(member.status().error, None);
        }
    }
}

fn channel_id(workspace: &Workspace, name: &str) -> String {
    workspace
        .channels()
        .unwrap()
        .into_iter()
        .find(|channel| channel.name == name)
        .unwrap()
        .id
}

#[tokio::test(flavor = "multi_thread")]
async fn two_members_collaborate_then_one_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let bucket = Bucket::new(Arc::new(InMemory::new()));
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let general = channel_id(&admin, "general");
    admin
        .send_message(&general, "hello before bob joined", None)
        .unwrap();
    admin.sync().await;

    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;

    // History from before the join is readable thanks to the keyring.
    let seen_by_bob = bob.messages(&general, 50).unwrap();
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
    let messages = bob.messages(&general, 50).unwrap();
    let asked = messages.iter().find(|message| message.id == question).unwrap();
    assert_eq!(asked.reactions[0].count, 2);
    assert!(asked.reactions[0].mine);
    assert_eq!(
        messages.last().unwrap().reply_to.as_deref(),
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
        !bob.messages(&general, 50)
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
    let bucket = Bucket::new(Arc::new(InMemory::new()));
    let admin = create_admin(&dir, &bucket).await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "persist me", None).unwrap();
    drop(admin);

    let reopened = Workspace::open_with(&dir, bucket, Networking::Offline)
        .await
        .unwrap();
    assert_eq!(reopened.messages(&general, 10).unwrap()[0].text, "persist me");
    reopened.send_message(&general, "and keep writing", None).unwrap();
    assert_eq!(reopened.messages(&general, 10).unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn bucket_never_sees_plaintext() {
    let root = tempfile::tempdir().unwrap();
    let memory = Arc::new(InMemory::new());
    let bucket = Bucket::new(memory.clone());
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

mod hardening {
    use super::*;
    use crate::{
        crypto,
        ops::{Envelope, Op, OpBody},
    };
    use iroh::SecretKey;

    async fn admin_and_bob() -> (tempfile::TempDir, Bucket, Workspace, Workspace) {
        let root = tempfile::tempdir().unwrap();
        let bucket = Bucket::new(Arc::new(InMemory::new()));
        let admin = create_admin(&root.path().join("ada"), &bucket).await;
        let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
        sync_all(&[&admin, &bob]).await;
        (root, bucket, admin, bob)
    }

    fn current_key(workspace: &Workspace) -> (u64, crypto::EpochKey) {
        let state = workspace.state();
        let epoch = state.mls.epoch().unwrap();
        (epoch, state.store.epoch_key(epoch).unwrap().unwrap())
    }

    fn rename(lamport: u64, page: &str, title: &str) -> OpBody {
        OpBody {
            lamport,
            timestamp_ms: 0,
            op: Op::PageRename {
                id: page.into(),
                title: title.into(),
            },
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rejects_ops_from_non_members_even_with_the_key() {
        let (_root, _bucket, admin, _bob) = admin_and_bob().await;
        let (epoch, key) = current_key(&admin);
        let outsider = SecretKey::generate();
        let page = admin.pages().unwrap()[0].id.clone();
        let forged = crypto::seal(&outsider, &key, epoch, 1, &rename(10_000, &page, "pwned")).unwrap();
        assert!(admin.receive(forged).is_err());
        assert_eq!(admin.pages().unwrap()[0].title, "Getting started");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rejects_implausible_lamport_jumps() {
        let (_root, _bucket, admin, bob) = admin_and_bob().await;
        let (epoch, key) = current_key(&bob);
        let page = admin.pages().unwrap()[0].id.clone();
        let seq = bob.state().store.next_seq(&bob.me()).unwrap();
        let forged = crypto::seal(
            &bob.inner.identity,
            &key,
            epoch,
            seq,
            &rename(u64::MAX, &page, "forever"),
        )
        .unwrap();
        admin.receive(forged).unwrap();
        assert_eq!(
            admin.pages().unwrap()[0].title,
            "Getting started",
            "far-future op must wait, not apply"
        );
        admin.rename_page(&page, "Still editable").unwrap();
        assert_eq!(admin.pages().unwrap()[0].title, "Still editable");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn junk_in_an_ops_slot_does_not_shadow_the_real_op() {
        let (_root, _bucket, admin, bob) = admin_and_bob().await;
        let general = channel_id(&admin, "general");
        let next_seq = bob.state().store.next_seq(&bob.me()).unwrap();
        let (epoch, _) = current_key(&bob);
        let junk = Envelope {
            author: bob.me(),
            seq: next_seq,
            epoch,
            nonce: [0; 24],
            ciphertext: vec![1, 2, 3],
        };
        assert!(admin.receive(junk).is_err());

        bob.send_message(&general, "the real message", None).unwrap();
        sync_all(&[&bob, &admin]).await;
        assert!(
            admin
                .messages(&general, 50)
                .unwrap()
                .iter()
                .any(|message| message.text == "the real message")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn garbage_commit_does_not_stall_sync_and_admin_repairs_it() {
        let (_root, bucket, admin, bob) = admin_and_bob().await;
        let general = channel_id(&admin, "general");
        let bob_epoch = bob.state().mls.epoch().unwrap();
        bucket.put_commit(bob_epoch, b"garbage".to_vec()).await.unwrap();

        admin.send_message(&general, "still flowing", None).unwrap();
        admin.sync().await;
        bob.sync().await;
        assert!(
            bob.messages(&general, 50)
                .unwrap()
                .iter()
                .any(|message| message.text == "still flowing")
        );
        assert!(bob.status().error.unwrap().contains("invalid commit"));

        // A real membership change: the admin's commit replaces the garbage.
        let root = tempfile::tempdir().unwrap();
        let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
        sync_all(&[&admin, &bob, &carol]).await;
        assert_eq!(
            bob.state().mls.epoch().unwrap(),
            admin.state().mls.epoch().unwrap()
        );
        carol.send_message(&general, "hi from carol", None).unwrap();
        sync_all(&[&carol, &bob]).await;
        assert!(
            bob.messages(&general, 50)
                .unwrap()
                .iter()
                .any(|message| message.text == "hi from carol")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn members_cannot_hijack_each_others_messages() {
        let (_root, _bucket, admin, bob) = admin_and_bob().await;
        let general = channel_id(&admin, "general");
        let original = admin.send_message(&general, "admin's words", None).unwrap();
        let (epoch, key) = current_key(&bob);
        let seq = bob.state().store.next_seq(&bob.me()).unwrap();
        let body = OpBody {
            lamport: 1,
            timestamp_ms: 0,
            op: Op::Message {
                id: original.clone(),
                channel: general.clone(),
                text: "forged".into(),
                reply_to: None,
            },
        };
        let forged = crypto::seal(&bob.inner.identity, &key, epoch, seq, &body).unwrap();
        assert!(admin.receive(forged).is_err());
        let message = admin
            .messages(&general, 50)
            .unwrap()
            .into_iter()
            .find(|m| m.id == original)
            .unwrap();
        assert_eq!(message.text, "admin's words");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn parked_junk_cannot_break_our_own_sequence() {
        let (_root, _bucket, _admin, bob) = admin_and_bob().await;
        let general = channel_id(&bob, "general");
        let (epoch, _) = current_key(&bob);
        let parked = |seq| Envelope {
            author: bob.me(),
            seq,
            epoch: epoch + 1,
            nonce: [0; 24],
            ciphertext: vec![0],
        };
        bob.receive(parked(1 << 40)).unwrap();
        assert!(bob.receive(parked(u64::MAX)).is_err());
        bob.send_message(&general, "still works", None).unwrap();
        assert!(
            bob.messages(&general, 50)
                .unwrap()
                .iter()
                .any(|message| message.text == "still works")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn restart_forgets_a_staged_commit_that_was_never_recorded() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("ada");
        let bucket = Bucket::new(Arc::new(InMemory::new()));
        let admin = create_admin(&dir, &bucket).await;
        {
            let mut state = admin.state();
            let identity = SecretKey::generate();
            let joiner = crate::mls::Mls::open(&root.path().join("j.db"), &identity, None, None).unwrap();
            let binding = identity.sign(&crate::mls::binding_message(joiner.signer_public()));
            let (_, key_package) = state
                .mls
                .verify_key_package(&joiner.key_package().unwrap(), &binding.to_bytes())
                .unwrap();
            state.mls.add_member(key_package).unwrap(); // staged, then "crash"
        }
        drop(admin);
        let reopened = Workspace::open_with(&dir, bucket, Networking::Offline)
            .await
            .unwrap();
        let state = reopened.state();
        assert_eq!(state.mls.epoch().unwrap(), 0);
        assert!(!state.mls.has_pending_commit());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pending_ops_are_capped() {
        let (_root, _bucket, admin, bob) = admin_and_bob().await;
        let (epoch, _) = current_key(&admin);
        for seq in 0..2_100u64 {
            let junk = Envelope {
                author: bob.me(),
                seq: 1_000_000 + seq,
                epoch: epoch + 1,
                nonce: [0; 24],
                ciphertext: vec![],
            };
            admin.receive(junk).unwrap();
        }
        assert_eq!(admin.state().store.pending().unwrap().len(), 2_000);
    }
}
