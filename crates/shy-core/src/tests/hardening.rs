//! Attacks from members, removed members and anyone with bucket access.
//! Every security rule in docs/security.md has a test here.

use iroh::SecretKey;

use super::support::*;
use crate::{
    DataDir, KeyStorage, Networking, Workspace, crypto,
    ops::{Envelope, Op, OpBody},
};

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
            .messages(&general, None, 50)
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
        bob.messages(&general, None, 50)
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
        bob.messages(&general, None, 50)
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
        .messages(&general, None, 50)
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
        bob.messages(&general, None, 50)
            .unwrap()
            .iter()
            .any(|message| message.text == "still works")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_forgets_a_staged_commit_that_was_never_recorded() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("ada");
    let bucket = memory_bucket();
    let admin = create_admin(&dir, &bucket).await;
    {
        let mut state = admin.state();
        let identity = SecretKey::generate();
        let joiner = crate::membership::mls::Mls::open(
            &root.path().join("j.db"),
            crate::storage::TEST_DB_KEY,
            &identity,
            None,
            None,
        )
        .unwrap();
        let binding = identity.sign(&crate::membership::mls::binding_message(joiner.signer_public()));
        let (_, key_package) = state
            .mls
            .verify_key_package(&joiner.key_package().unwrap(), &binding.to_bytes())
            .unwrap();
        state.mls.add_member(key_package).unwrap(); // staged, then "crash"
    }
    drop(admin);
    let reopened = Workspace::open_with(&data_dir(&dir), |_| Ok(bucket), Networking::Offline)
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

#[tokio::test(flavor = "multi_thread")]
async fn removed_members_cannot_backdate_ops_with_old_keys() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "written before removal", None)
        .unwrap();
    bob.sync().await;

    let (old_epoch, old_key) = current_key(&bob);
    let next_seq = bob.state().store.next_seq(&bob.me()).unwrap();
    admin.remove_member(&bob.my_id()).await.unwrap();
    sync_all(&[&admin, &carol]).await;
    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    sync_all(&[&admin, &carol, &dave]).await;

    let body = OpBody {
        lamport: 1_000,
        timestamp_ms: 0,
        op: Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: "forged after removal".into(),
            reply_to: None,
        },
    };
    let forged = crypto::seal(&bob.inner.identity, &old_key, old_epoch, next_seq, &body).unwrap();
    for member in [&admin, &carol, &dave] {
        assert!(member.receive(forged.clone()).is_err());
        let texts: Vec<_> = member
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .map(|m| m.text)
            .collect();
        assert!(texts.contains(&"written before removal".to_string()));
        assert!(!texts.contains(&"forged after removal".to_string()));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_data_directory_alone_reveals_nothing() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("ada");
    let admin = create_admin(&dir, &memory_bucket()).await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "LOCAL-SECRET-TEXT", None).unwrap();
    let identity = admin.inner.identity.to_bytes();
    drop(admin);

    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|window| window == needle);
        assert!(
            !contains(b"LOCAL-SECRET-TEXT"),
            "message text in {}",
            path.display()
        );
        assert!(
            !contains(b"test-bucket-secret"),
            "bucket secret in {}",
            path.display()
        );
        assert!(!contains(&identity), "identity key in {}", path.display());
    }

    let plain = rusqlite::Connection::open(dir.join("shy.db")).unwrap();
    assert!(
        plain
            .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .is_err()
    );

    let wrong_key = DataDir::new(&dir, KeyStorage::File(root.path().join("other.key")));
    let error = Workspace::open_with(&wrong_key, |_| Ok(memory_bucket()), Networking::Offline)
        .await
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("can't be decrypted"), "{error:#}");
}
