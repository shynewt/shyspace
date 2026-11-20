//! Attacks from members, removed members and anyone with bucket access.
//! Every security rule in docs/security.md has a test here.

use iroh::SecretKey;

use super::support::*;
use crate::{
    DataDir, KeyStorage, Networking, Workspace, crypto,
    ops::{Envelope, Op, OpBody, now_ms},
    views::PresenceState,
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
    admin.receive(forged).unwrap(); // skipped as unappliable, never applied
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
    let reopened = Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline)
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
    let error = Workspace::open_with(&wrong_key, connector(&memory_bucket()), Networking::Offline)
        .await
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("can't be decrypted"), "{error:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn gaps_in_a_log_cannot_be_filled_after_removal() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let (epoch, key) = current_key(&bob);
    let next_seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let message = |text: &str| OpBody {
        lamport: 5_000,
        timestamp_ms: 0,
        op: Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: text.into(),
            reply_to: None,
        },
    };
    // Before removal, Bob publishes far ahead, leaving a gap to fill later.
    let far_ahead = crypto::seal(
        &bob.inner.identity,
        &key,
        epoch,
        next_seq + 1_000,
        &message("far ahead"),
    )
    .unwrap();
    admin.receive(far_ahead).unwrap();

    admin.remove_member(&bob.my_id()).await.unwrap();
    let in_gap = crypto::seal(
        &bob.inner.identity,
        &key,
        epoch,
        next_seq,
        &message("slipped into the gap"),
    )
    .unwrap();
    assert!(admin.receive(in_gap).is_err());
    admin.sync().await;

    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(
        !texts
            .iter()
            .any(|text| text == "far ahead" || text == "slipped into the gap")
    );
    assert!(
        admin.state().store.pending().unwrap().is_empty(),
        "the parked op is dropped once frozen"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_op_that_cannot_apply_does_not_block_later_ones() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let (epoch, key) = current_key(&bob);
    let seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let message = |id: String, text: &str| OpBody {
        lamport: 9_000,
        timestamp_ms: 0,
        op: Op::Message {
            id,
            channel: general.clone(),
            text: text.into(),
            reply_to: None,
        },
    };
    let claims_admins_id = message(crate::ops::new_owned_id(&admin.me()), "bad");
    let good = message(crate::ops::new_owned_id(&bob.me()), "good after bad");
    let bad = crypto::seal(&bob.inner.identity, &key, epoch, seq, &claims_admins_id).unwrap();
    let next = crypto::seal(&bob.inner.identity, &key, epoch, seq + 1, &good).unwrap();

    admin.receive(next).unwrap();
    admin.receive(bad).unwrap();
    admin.sync().await;
    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(texts.contains(&"good after bad".to_string()));
    assert!(!texts.contains(&"bad".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn forged_edits_and_deletes_of_someone_elses_content_are_skipped() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "the real text", None).unwrap();
    let page = admin.create_page("Plans", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let (epoch, key) = current_key(&bob);
    let first_seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let forgeries = [
        Op::MessageEdit {
            id: message.clone(),
            text: "forged".into(),
        },
        Op::Delete { id: message.clone() },
        Op::Delete { id: general.clone() },
        Op::Delete { id: page.clone() },
    ];
    for (offset, op) in forgeries.into_iter().enumerate() {
        let body = OpBody {
            lamport: 9_000 + offset as u64,
            timestamp_ms: 0,
            op,
        };
        let seq = first_seq + offset as u64;
        admin
            .receive(crypto::seal(&bob.inner.identity, &key, epoch, seq, &body).unwrap())
            .unwrap();
    }
    admin.sync().await;

    let messages = admin.messages(&general, None, 50).unwrap();
    let kept = messages.iter().find(|candidate| candidate.id == message).unwrap();
    assert_eq!(kept.text, "the real text");
    assert!(!kept.is_edited);
    assert!(
        admin
            .channels()
            .unwrap()
            .iter()
            .any(|channel| channel.id == general)
    );
    assert!(
        admin
            .pages()
            .unwrap()
            .iter()
            .any(|candidate| candidate.id == page)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_and_deletes_must_name_an_item_of_the_right_kind() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let page = admin.create_page("Plans", None).unwrap();
    let message = admin.send_message(&general, "hello", None).unwrap();

    assert!(
        admin.delete_message(&general).is_err(),
        "a channel id isn't a message"
    );
    assert!(
        admin.edit_message(&page, "text").is_err(),
        "a page id isn't a message"
    );
    assert!(admin.delete_page(&message).is_err(), "a message id isn't a page");
    assert!(admin.delete_channel(&page).is_err(), "a page id isn't a channel");
    assert!(admin.edit_message("missing", "text").is_err());
    assert!(
        admin
            .channels()
            .unwrap()
            .iter()
            .any(|channel| channel.id == general)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn channel_names_from_others_must_be_plain_and_short() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let (epoch, key) = current_key(&bob);
    let first_seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let long_name = "x".repeat(10_000);
    let renames = [
        "fine-name",
        "\u{202e}lanreg",
        "",
        long_name.as_str(),
        "line\nbreak",
    ];
    let mut ops: Vec<Op> = renames
        .into_iter()
        .map(|name| Op::ChannelRename {
            id: general.clone(),
            name: name.into(),
        })
        .collect();
    ops.push(Op::Message {
        id: crate::ops::new_owned_id(&bob.me()),
        channel: general.clone(),
        text: "still delivered".into(),
        reply_to: None,
    });
    for (offset, op) in ops.into_iter().enumerate() {
        let body = OpBody {
            lamport: 9_000 + offset as u64,
            timestamp_ms: 0,
            op,
        };
        let seq = first_seq + offset as u64;
        admin
            .receive(crypto::seal(&bob.inner.identity, &key, epoch, seq, &body).unwrap())
            .unwrap();
    }

    let names: Vec<_> = admin
        .channels()
        .unwrap()
        .into_iter()
        .map(|channel| channel.name)
        .collect();
    assert_eq!(
        names,
        vec!["fine-name".to_string()],
        "later, invalid names are skipped"
    );
    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(texts.contains(&"still delivered".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn ops_from_a_newer_version_wait_instead_of_being_dropped() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let (epoch, key) = current_key(&bob);
    let seq = bob.state().store.next_seq(&bob.me()).unwrap();
    // An op body whose kind of change this version doesn't know.
    let mut unknown_kind = postcard::to_stdvec(&(9_000u64, 0i64)).unwrap();
    unknown_kind.push(100);
    let from_the_future = crypto::seal_bytes(&bob.inner.identity, &key, epoch, seq, unknown_kind).unwrap();
    let next = OpBody {
        lamport: 9_001,
        timestamp_ms: 0,
        op: Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: "after the new kind".into(),
            reply_to: None,
        },
    };
    let next = crypto::seal(&bob.inner.identity, &key, epoch, seq + 1, &next).unwrap();

    admin.receive(from_the_future).unwrap();
    admin.receive(next).unwrap();
    admin.sync().await;

    assert_eq!(
        admin.state().store.pending().unwrap().len(),
        2,
        "both wait for an update"
    );
    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(!texts.contains(&"after the new kind".to_string()));
    assert!(admin.status().needs_update, "the app says to update");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_op_far_ahead_raises_no_update_alarm() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let (epoch, key) = current_key(&bob);
    let far_ahead = bob.state().store.next_seq(&bob.me()).unwrap() + 500;
    let mut junk = postcard::to_stdvec(&(9_000u64, 0i64)).unwrap();
    junk.push(100);
    admin
        .receive(crypto::seal_bytes(&bob.inner.identity, &key, epoch, far_ahead, junk).unwrap())
        .unwrap();
    admin.sync().await;

    let status = admin.status();
    assert!(!status.needs_update);
    assert_eq!(status.error, None, "sync still reports success");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_op_is_fetched_again_from_the_bucket() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "second", None).unwrap();
    bob.send_message(&general, "third", None).unwrap();
    let unpushed = bob.state().store.unpushed(&bob.me()).unwrap();
    let third = unpushed.last().unwrap().clone();
    bob.sync().await;

    // As if the admin had read past "second" in the bucket and then lost it.
    admin.state().store.set_cursor(&bob.me(), "z").unwrap();
    admin.receive(third).unwrap();
    admin.sync().await;
    admin.sync().await;

    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(
        texts.contains(&"second".to_string()) && texts.contains(&"third".to_string()),
        "{texts:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn channels_named_before_the_naming_rule_still_show_for_everyone() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let (epoch, key) = current_key(&bob);
    let first_seq = bob.state().store.next_seq(&bob.me()).unwrap();
    for (offset, name) in ["q&a", "\u{202e}evil\u{200b}name"].into_iter().enumerate() {
        let body = OpBody {
            lamport: 9_000 + offset as u64,
            timestamp_ms: 0,
            op: Op::ChannelCreate {
                id: crate::ops::new_owned_id(&bob.me()),
                name: name.into(),
            },
        };
        let seq = first_seq + offset as u64;
        admin
            .receive(crypto::seal(&bob.inner.identity, &key, epoch, seq, &body).unwrap())
            .unwrap();
    }

    let names: Vec<_> = admin
        .channels()
        .unwrap()
        .into_iter()
        .map(|channel| channel.name)
        .collect();
    assert!(names.contains(&"q&a".to_string()), "{names:?}");
    assert!(
        names.contains(&"evilname".to_string()),
        "invisible characters are dropped: {names:?}"
    );
}

fn member_presence(workspace: &Workspace, id: &str) -> PresenceState {
    workspace
        .members()
        .unwrap()
        .into_iter()
        .find(|member| member.id == id)
        .unwrap()
        .presence
}

#[tokio::test(flavor = "multi_thread")]
async fn statuses_from_others_are_bounded() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    assert!(
        bob.set_status(crate::views::StatusUpdate {
            emoji: "x",
            text: &"y".repeat(500),
            until_ms: None,
            alerts: crate::views::NotifyLevel::All
        })
        .is_err()
    );

    let (epoch, key) = current_key(&bob);
    let seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let body = OpBody {
        lamport: 9_000,
        timestamp_ms: 0,
        op: Op::Status {
            emoji: "x".into(),
            text: "y".repeat(5_000),
            until_ms: None,
            alerts: crate::ops::StatusAlerts::All,
        },
    };
    admin
        .receive(crypto::seal(&bob.inner.identity, &key, epoch, seq, &body).unwrap())
        .unwrap();
    assert!(
        admin
            .members()
            .unwrap()
            .into_iter()
            .find(|member| member.id == bob.my_id())
            .unwrap()
            .status
            .is_none(),
        "an oversized status is skipped"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn heartbeats_must_be_signed_fresh_and_from_a_member() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let mut forged = bob.heartbeat(PresenceState::Online).unwrap();
    forged.member = admin.me();
    assert!(
        bob.receive_heartbeat(&forged).is_err(),
        "a heartbeat must be signed by its member"
    );
    let mut tampered = bob.heartbeat(PresenceState::Away).unwrap();
    tampered.state = PresenceState::Online;
    assert!(
        admin.receive_heartbeat(&tampered).is_err(),
        "the state is signed too"
    );
    let stale = bob
        .heartbeat_at(PresenceState::Online, now_ms().unwrap() - 10 * 60 * 1000)
        .unwrap();
    assert!(
        admin.receive_heartbeat(&stale).is_err(),
        "an old heartbeat, even correctly signed, is refused"
    );
    assert_eq!(member_presence(&admin, &bob.my_id()), PresenceState::Unreachable);

    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let from_carol = carol.heartbeat(PresenceState::Online).unwrap();
    admin.remove_member(&carol.my_id()).await.unwrap();
    assert!(
        admin.receive_heartbeat(&from_carol).is_err(),
        "a removed member can't appear online"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn statuses_cannot_hide_or_reorder_text() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    for sneaky in [
        "\u{202E}krow peed",
        "a\u{200B}b",
        "line\u{2028}break",
        "\u{2066}isolated",
    ] {
        let status = crate::views::StatusUpdate {
            emoji: "",
            text: sneaky,
            until_ms: None,
            alerts: crate::views::NotifyLevel::All,
        };
        assert!(bob.set_status(status).is_err(), "{sneaky:?} must be refused");
    }
    let joined_emoji = crate::views::StatusUpdate {
        emoji: "👩\u{200D}💻",
        text: "Coding",
        until_ms: None,
        alerts: crate::views::NotifyLevel::All,
    };
    bob.set_status(joined_emoji)
        .expect("emoji built with joiners are fine");

    let (epoch, key) = current_key(&bob);
    let seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let body = OpBody {
        lamport: 9_000,
        timestamp_ms: 0,
        op: Op::Status {
            emoji: String::new(),
            text: "\u{202E}evil".into(),
            until_ms: None,
            alerts: crate::ops::StatusAlerts::All,
        },
    };
    admin
        .receive(crypto::seal(&bob.inner.identity, &key, epoch, seq, &body).unwrap())
        .unwrap();
    let status = admin
        .members()
        .unwrap()
        .into_iter()
        .find(|member| member.id == bob.my_id())
        .unwrap()
        .status;
    assert!(
        status.is_none(),
        "a direction override from another member is skipped"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rapid_presence_flips_are_ignored() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let now = now_ms().unwrap();
    assert!(
        admin
            .receive_heartbeat(&bob.heartbeat_at(PresenceState::Online, now - 2).unwrap())
            .unwrap()
    );
    let flip = bob.heartbeat_at(PresenceState::Away, now - 1).unwrap();
    assert!(
        !admin.receive_heartbeat(&flip).unwrap(),
        "a flip right after an accepted heartbeat changes nothing"
    );
    assert_eq!(member_presence(&admin, &bob.my_id()), PresenceState::Online);

    // A real change a moment later still gets through at once.
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    let back = bob.heartbeat(PresenceState::Away).unwrap();
    assert!(admin.receive_heartbeat(&back).unwrap());
    assert_eq!(member_presence(&admin, &bob.my_id()), PresenceState::Away);
}
