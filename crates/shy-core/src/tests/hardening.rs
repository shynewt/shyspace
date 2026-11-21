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
async fn ops_applied_just_before_a_removal_arrives_are_undone() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let earlier = bob
        .send_message(&general, "written before removal", None)
        .unwrap();
    let page = bob.create_page("Plans", None).unwrap();
    bob.edit_page(&page, crate::Edit::Append("agreed plan")).unwrap();
    bob.sync().await;
    carol.sync().await;
    carol.set_draft(&general, "unsent thought").unwrap();
    carol.mark_read(&general, &earlier).unwrap();

    admin.remove_member(&bob.my_id()).await.unwrap();
    // Bob gets a last word in live, before Carol hears about the removal:
    // a new message, a delete of his earlier one, and a page edit.
    let mut forger = Forger::new(&bob);
    let last_word = crate::ops::new_owned_id(&bob.me());
    let late_edit = crate::pages::edit(
        carol.page_state(&page).unwrap().as_deref(),
        crate::Edit::Append("SNEAKED-INTO-PAGE"),
    )
    .unwrap();
    for op in [
        Op::Message {
            id: last_word.clone(),
            channel: general.clone(),
            text: "SENT-AFTER-REMOVAL".into(),
            reply_to: None,
        },
        Op::Delete { id: earlier.clone() },
        Op::DocUpdate {
            page: page.clone(),
            update: late_edit,
        },
    ] {
        carol.receive(forger.seal(op)).unwrap();
    }
    carol.react(&last_word, "👍", true).unwrap();
    let texts = texts_in(&carol, &general);
    assert!(texts.contains(&"SENT-AFTER-REMOVAL".to_string()));
    assert!(!texts.contains(&"written before removal".to_string()));
    assert!(carol.page_text(&page).unwrap().contains("SNEAKED-INTO-PAGE"));

    carol.sync().await;
    admin.sync().await;
    for member in [&admin, &carol] {
        let texts = texts_in(member, &general);
        assert!(texts.contains(&"written before removal".to_string()));
        assert!(!texts.contains(&"SENT-AFTER-REMOVAL".to_string()));
        assert!(member.search("SENT-AFTER-REMOVAL", 10).unwrap().is_empty());
        assert_eq!(member.page_text(&page).unwrap(), "agreed plan");
    }
    assert_eq!(shared_view(&carol, &general), shared_view(&admin, &general));
    assert_eq!(
        carol.state().store.last_seq(&bob.me()).unwrap(),
        admin.state().store.last_seq(&bob.me()).unwrap(),
        "Carol's copy of Bob's log ends where the removal froze it"
    );
    assert_eq!(carol.drafts().unwrap().len(), 1, "drafts are kept");
    assert!(
        carol
            .channels()
            .unwrap()
            .iter()
            .any(|channel| channel.id == general && channel.unread == 0),
        "read markers are kept"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_page_update_cannot_stop_a_device_following_a_removal() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let mut forger = Forger::new(&bob);
    // Planted earlier, so it is part of Bob's log that everyone keeps.
    let damaged = forger.seal(Op::DocUpdate {
        page: crate::ops::new_owned_id(&bob.me()),
        update: b"not a yjs update".to_vec(),
    });
    for member in [&admin, &carol] {
        member.receive(damaged.clone()).unwrap();
    }

    admin.remove_member(&bob.my_id()).await.unwrap();
    carol
        .receive(forger.seal(Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: "SENT-AFTER-REMOVAL".into(),
            reply_to: None,
        }))
        .unwrap();
    carol.sync().await;

    assert_eq!(carol.status().error, None);
    assert_eq!(
        current_key(&carol).0,
        current_key(&admin).0,
        "Carol reached the new epoch"
    );
    assert!(!texts_in(&carol, &general).contains(&"SENT-AFTER-REMOVAL".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_sealing_keys_from_others_are_skipped() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let announced = admin.state().store.sealing_key_of(&bob.me()).unwrap();
    let general = channel_id(&admin, "general");
    let mut forger = Forger::new(&bob);
    for public in [vec![], vec![7; 31], vec![7; 33]] {
        admin.receive(forger.seal(Op::SealingKey { public })).unwrap();
    }
    admin
        .receive(forger.seal(Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: "still heard".into(),
            reply_to: None,
        }))
        .unwrap();
    assert_eq!(admin.state().store.sealing_key_of(&bob.me()).unwrap(), announced);
    assert!(texts_in(&admin, &general).contains(&"still heard".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn ordinary_ops_cannot_reach_private_ids() {
    use crate::private::ids::{new_private_message_id, private_channel_id};
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let private = private_channel_id(&bob.me(), &[0; 32], "secret", &[]).unwrap();
    let message = new_private_message_id(&private, &bob.me());
    let mut forger = Forger::new(&bob);
    for op in [
        Op::ChannelCreate {
            id: private.clone(),
            name: "secret".into(),
        },
        Op::Message {
            id: message.clone(),
            channel: private.clone(),
            text: "PLAIN-INTO-PRIVATE".into(),
            reply_to: None,
        },
        Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: "PLAIN-REPLY-TO-PRIVATE".into(),
            reply_to: Some(message.clone()),
        },
        Op::Reaction {
            message: message.clone(),
            emoji: "👍".into(),
            active: true,
        },
    ] {
        admin.receive(forger.seal(op)).unwrap();
    }
    assert!(admin.messages(&private, None, 50).unwrap().is_empty());
    assert!(!texts_in(&admin, &general).contains(&"PLAIN-REPLY-TO-PRIVATE".to_string()));
    assert!(
        admin
            .channels()
            .unwrap()
            .iter()
            .all(|channel| channel.id != private)
    );
    assert_eq!(
        admin.state().store.count_where("reactions", "message", &message),
        0
    );

    // Not even the admin can delete in a channel they can't see.
    bob.receive(Forger::new(&admin).seal(Op::Delete { id: private.clone() }))
        .unwrap();
    assert!(!bob.state().store.is_deleted(&private).unwrap());
}

fn texts_in(member: &Workspace, channel: &str) -> Vec<String> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect()
}

/// What every member should see the same, leaving out personal state.
fn shared_view(member: &Workspace, channel: &str) -> Vec<(String, String, bool, u64)> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| (message.id, message.text, message.is_edited, message.lamport))
        .collect()
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

#[tokio::test(flavor = "multi_thread")]
async fn huge_text_from_others_is_cut_to_the_limit() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = crate::ops::new_owned_id(&bob.me());
    let huge = "é".repeat(100_000);
    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal(Op::Message {
            id: message.clone(),
            channel: general.clone(),
            text: huge.clone(),
            reply_to: None,
        }))
        .unwrap();
    let stored = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|m| m.id == message)
        .unwrap();
    assert_eq!(stored.text.chars().count(), crate::ops::MAX_MESSAGE_CHARS);

    admin
        .receive(forger.seal(Op::MessageEdit {
            id: message.clone(),
            text: "ü".repeat(100_000),
        }))
        .unwrap();
    let edited = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|m| m.id == message)
        .unwrap();
    assert!(edited.text.starts_with('ü'), "the edit applied");
    assert_eq!(edited.text.chars().count(), crate::ops::MAX_MESSAGE_CHARS);

    let page = crate::ops::new_owned_id(&bob.me());
    admin
        .receive(forger.seal(Op::PageCreate {
            id: page.clone(),
            parent: None,
            title: huge.clone(),
        }))
        .unwrap();
    let title = || {
        admin
            .pages()
            .unwrap()
            .into_iter()
            .find(|p| p.id == page)
            .unwrap()
            .title
    };
    assert_eq!(title().chars().count(), crate::ops::MAX_PAGE_TITLE_CHARS);
    admin
        .receive(forger.seal(Op::PageRename {
            id: page.clone(),
            title: huge,
        }))
        .unwrap();
    assert_eq!(title().chars().count(), crate::ops::MAX_PAGE_TITLE_CHARS);
}

#[tokio::test(flavor = "multi_thread")]
async fn your_own_text_over_the_limit_is_refused() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let at_limit = format!("  {}  ", "a".repeat(crate::ops::MAX_MESSAGE_CHARS));
    admin
        .send_message(&general, &at_limit, None)
        .expect("spaces around it don't count");
    admin
        .create_page(&"t".repeat(crate::ops::MAX_PAGE_TITLE_CHARS), None)
        .expect("a title at the limit is fine");
    let too_long = "a".repeat(crate::ops::MAX_MESSAGE_CHARS + 1);
    assert!(admin.send_message(&general, &too_long, None).is_err());
    let message = admin
        .send_message(&general, &"a".repeat(crate::ops::MAX_MESSAGE_CHARS), None)
        .unwrap();
    assert!(admin.edit_message(&message, &too_long).is_err());
    assert!(
        admin
            .create_page(&"t".repeat(crate::ops::MAX_PAGE_TITLE_CHARS + 1), None)
            .is_err()
    );
    let page = admin.create_page("Spec", None).unwrap();
    assert!(
        admin
            .rename_page(&page, &"t".repeat(crate::ops::MAX_PAGE_TITLE_CHARS + 1))
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mention_hidden_past_the_limit_does_not_alert() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mention = crate::mentions::member_link("Ada", &admin.my_id());
    let hidden = format!("{}{mention}", "a".repeat(crate::ops::MAX_MESSAGE_CHARS));
    admin
        .set_notify_level(&general, crate::views::NotifyLevel::Mentions)
        .unwrap();
    let mut changes = admin.subscribe();
    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal(Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: hidden,
            reply_to: None,
        }))
        .unwrap();
    while let Ok(change) = changes.try_recv() {
        assert!(
            !matches!(change, crate::views::Change::Alert { .. }),
            "nobody can see that mention"
        );
    }

    admin
        .receive(forger.seal(Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general,
            text: format!("{mention} visible"),
            reply_to: None,
        }))
        .unwrap();
    let mut alerts = 0;
    while let Ok(change) = changes.try_recv() {
        alerts += usize::from(matches!(change, crate::views::Change::Alert { .. }));
    }
    assert_eq!(alerts, 1, "a mention people can see still alerts");
}

#[tokio::test(flavor = "multi_thread")]
async fn names_and_emoji_from_others_are_bounded_and_plain() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "react to me", None).unwrap();
    let mut forger = Forger::new(&bob);
    let disguised = format!("Ada\u{202E}nimda{}", "x".repeat(10_000));
    admin
        .receive(forger.seal(Op::Profile {
            name: disguised,
            is_bot: false,
        }))
        .unwrap();
    admin
        .receive(forger.seal(Op::Reaction {
            message: message.clone(),
            emoji: format!("👍\u{0007}{}", "🎉".repeat(5_000)),
            active: true,
        }))
        .unwrap();

    let name = admin
        .members()
        .unwrap()
        .into_iter()
        .find(|member| member.id == bob.my_id())
        .unwrap()
        .name;
    assert!(name.chars().count() <= 80, "{} characters", name.chars().count());
    assert!(!name.contains('\u{202E}'));
    let messages = admin.messages(&general, None, 50).unwrap();
    let reactions = &messages.last().unwrap().reactions;
    assert_eq!(reactions.len(), 1);
    assert!(reactions[0].emoji.chars().count() <= 32);
    assert!(!reactions[0].emoji.contains('\u{0007}'));
}

#[tokio::test(flavor = "multi_thread")]
async fn ops_naming_huge_ids_are_skipped_and_huge_parents_dropped() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal(Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: "reply to a giant".into(),
            reply_to: Some("x".repeat(100_000)),
        }))
        .unwrap();
    admin
        .receive(forger.seal(Op::Reaction {
            message: "y".repeat(100_000),
            emoji: "👍".into(),
            active: true,
        }))
        .unwrap();
    // Shown as a plain message, as older versions show a reply whose parent is missing.
    let reply = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|message| message.text == "reply to a giant")
        .unwrap();
    assert_eq!(reply.reply_to, None);
    assert_eq!(
        admin
            .state()
            .store
            .count_where("reactions", "message", &"y".repeat(100_000)),
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_huge_page_is_only_partly_searchable() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = bob.create_page("Big", None).unwrap();
    let filler = "lorem ".repeat(50_000);
    bob.edit_page(&page, crate::Edit::Append(&format!("{filler}NEEDLEATTHEEND")))
        .unwrap();
    bob.sync().await;
    admin.sync().await;
    assert!(admin.search("NEEDLEATTHEEND", 10).unwrap().is_empty());
    assert!(!admin.search("lorem", 10).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn your_own_name_and_emoji_over_the_limit_are_refused() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "hi", None).unwrap();
    let too_long = "a".repeat(81);
    assert!(
        admin
            .set_profile(crate::Profile {
                name: &too_long,
                is_bot: false,
            })
            .is_err()
    );
    assert!(
        admin
            .set_profile(crate::Profile {
                name: "Ada\u{202E}",
                is_bot: false,
            })
            .is_err()
    );
    assert!(admin.react(&message, &"🎉".repeat(33), true).is_err());
    let longest = "a".repeat(80);
    admin
        .set_profile(crate::Profile {
            name: &longest,
            is_bot: false,
        })
        .unwrap();
    admin.react(&message, "👍🏽", true).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn names_stored_by_an_older_version_are_cleaned_once_on_upgrade() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let dir = root.path().join("ada");
    let admin = create_admin(&dir, &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;
    admin
        .receive(Forger::new(&bob).seal(Op::Profile {
            name: "Bob\u{202E}nimda".into(),
            is_bot: false,
        }))
        .unwrap();
    // As an older version stored it: whole, and with no record of the rules.
    admin.state().store.execute_for_test(
        "UPDATE profiles SET name = 'Bob' || char(8238) || 'nimda' WHERE name LIKE 'Bobnimda%';
         DELETE FROM meta WHERE key = 'views_version';",
    );
    let name_of_bob = |member: &Workspace| {
        member
            .members()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == bob.my_id())
            .unwrap()
            .name
    };
    assert!(
        name_of_bob(&admin).contains('\u{202E}'),
        "the old state is set up"
    );
    drop(admin);

    let reopened = Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    assert_eq!(name_of_bob(&reopened), "Bobnimda");
}

fn details_of(member: &Workspace, id: &str) -> crate::profile::ProfileDetails {
    member
        .members()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == id)
        .unwrap()
        .details
}

fn titled(title: &str) -> crate::profile::ProfileDetails {
    crate::profile::ProfileDetails {
        title: title.into(),
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_details_from_others_are_bounded_plain_and_safe_to_open() {
    use crate::profile::{
        MAX_BIO_CHARS, MAX_LOCATION_CHARS, MAX_PRONOUNS_CHARS, MAX_TITLE_CHARS, WorkingHours,
    };
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let hostile = crate::profile::ProfileDetails {
        title: format!("Ad\u{202E}min\u{0007}{}", "x".repeat(10_000)),
        pronouns: format!("\u{200B}{}", "p".repeat(10_000)),
        bio: format!("hello\u{2066}\r\n{}", "🎉".repeat(10_000)),
        email: format!("{}@example.com", "a".repeat(10_000)),
        phone: "javascript:alert(1)".into(),
        website: "javascript:alert(document.cookie)".into(),
        location: "l".repeat(10_000),
        time_zone: "Europe/Lisbon\u{202E}; DROP TABLE".into(),
        working_hours: Some(WorkingHours {
            days: 0xFF,
            start_minute: 9_000,
            end_minute: 60_000,
        }),
    };
    admin
        .receive(Forger::new(&bob).seal(Op::ProfileDetails { details: hostile }))
        .unwrap();

    let kept = details_of(&admin, &bob.my_id());
    assert!(kept.title.starts_with("Adminx"), "{}", kept.title);
    assert_eq!(kept.title.chars().count(), MAX_TITLE_CHARS);
    assert_eq!(kept.pronouns, "p".repeat(MAX_PRONOUNS_CHARS));
    assert!(kept.bio.starts_with("hello\n🎉"));
    assert_eq!(kept.bio.chars().count(), MAX_BIO_CHARS);
    assert_eq!(kept.location.chars().count(), MAX_LOCATION_CHARS);
    assert_eq!(
        (
            kept.email.as_str(),
            kept.phone.as_str(),
            kept.website.as_str(),
            kept.time_zone.as_str()
        ),
        ("", "", "", ""),
        "values that can't be cut or could open something else are cleared"
    );
    assert_eq!(kept.working_hours, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_http_websites_survive_from_others() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let mut forger = Forger::new(&bob);
    for website in [
        "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
        "JAVASCRIPT:alert(1)",
        "vbscript:msgbox",
        "file:///etc/passwd",
    ] {
        let details = crate::profile::ProfileDetails {
            website: website.into(),
            ..titled("Engineer")
        };
        admin
            .receive(forger.seal(Op::ProfileDetails { details }))
            .unwrap();
        let kept = details_of(&admin, &bob.my_id());
        assert_eq!(kept.website, "", "{website}");
        assert_eq!(kept.title, "Engineer", "the rest of the op still counts");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_profile_details_op_does_not_replace_newer_ones() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal_at(
            20_000,
            Op::ProfileDetails {
                details: titled("Current"),
            },
        ))
        .unwrap();
    admin
        .receive(forger.seal_at(
            10_000,
            Op::ProfileDetails {
                details: titled("Stale"),
            },
        ))
        .unwrap();
    assert_eq!(details_of(&admin, &bob.my_id()).title, "Current");
}

#[tokio::test(flavor = "multi_thread")]
async fn nobody_can_set_someone_elses_profile_details() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    bob.set_profile_details(&titled("Engineer")).unwrap();
    sync_all(&[&bob, &admin]).await;

    // Carol signs an op and claims Bob wrote it.
    let mut claimed = Forger::new(&carol).seal(Op::ProfileDetails {
        details: titled("Fired"),
    });
    claimed.author = bob.me();
    claimed.seq = bob.state().store.next_seq(&bob.me()).unwrap();
    assert!(admin.receive(claimed).is_err());
    assert_eq!(details_of(&admin, &bob.my_id()).title, "Engineer");

    // An op has no field naming whose details it sets: Carol's own op only sets hers.
    admin
        .receive(Forger::new(&carol).seal(Op::ProfileDetails {
            details: titled("Carol's title"),
        }))
        .unwrap();
    assert_eq!(details_of(&admin, &bob.my_id()).title, "Engineer");
    assert_eq!(details_of(&admin, &carol.my_id()).title, "Carol's title");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_member_cannot_change_their_profile_details() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    bob.set_profile_details(&titled("Engineer")).unwrap();
    sync_all(&[&bob, &admin]).await;
    assert_eq!(details_of(&admin, &bob.my_id()).title, "Engineer");

    let mut forger = Forger::new(&bob);
    admin.remove_member(&bob.my_id()).await.unwrap();
    let late = forger.seal(Op::ProfileDetails {
        details: titled("Still here"),
    });
    assert!(admin.receive(late).is_err());
    assert_eq!(details_of(&admin, &bob.my_id()).title, "Engineer");
}

#[tokio::test(flavor = "multi_thread")]
async fn your_own_profile_details_are_refused_when_malformed() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let website = crate::profile::ProfileDetails {
        website: "javascript:alert(1)".into(),
        ..Default::default()
    };
    assert!(admin.set_profile_details(&website).is_err());
    assert!(admin.set_profile_details(&titled(&"x".repeat(81))).is_err());
    assert_eq!(details_of(&admin, &admin.my_id()), Default::default());

    admin.set_profile_details(&titled("  Founder ")).unwrap();
    assert_eq!(details_of(&admin, &admin.my_id()).title, "Founder");
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_contacts_that_hide_where_they_go_are_cleared() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let mut forger = Forger::new(&bob);
    for (email, website) in [
        (
            "ceo\u{E0041}@corp.example",
            "https://accounts.google.com@evil.example/login",
        ),
        ("ceo\u{200B}@corp.example", "https://evil.example\\@google.com"),
        (
            "ceo@corp.example\u{E007F}",
            "https://google.com\u{E0041}.evil.example/",
        ),
    ] {
        let details = crate::profile::ProfileDetails {
            email: email.into(),
            website: website.into(),
            ..titled("Engineer")
        };
        admin
            .receive(forger.seal(Op::ProfileDetails { details }))
            .unwrap();
        let kept = details_of(&admin, &bob.my_id());
        assert_eq!(
            (kept.email.as_str(), kept.website.as_str()),
            ("", ""),
            "{email} {website}"
        );
        assert_eq!(kept.title, "Engineer");
    }
}
