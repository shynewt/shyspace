//! Attacks from members, removed members and anyone with bucket access.
//! Every security rule in docs/security.md has a test here.

use iroh::SecretKey;

use super::support::*;
use crate::{
    DataDir, KeyStorage, Networking, Workspace, crypto,
    membership::commit::{self, FrozenAuthor},
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

/// A page is one Yjs text, and an editor only appends its own new pieces:
/// updates shaped otherwise are refused before yrs sees them, so these
/// don't overflow the stack (nested values; clients with no pieces) on every
/// device that pulls them.
#[tokio::test(flavor = "multi_thread")]
async fn crafted_page_updates_cannot_crash_or_exhaust_a_device() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.pages().unwrap()[0].id.clone();
    let before = admin.page_text(&page).unwrap();
    let mut nested_arrays = vec![1, 1, 1, 0, 8, 1, 7];
    nested_arrays.extend_from_slice(b"content");
    nested_arrays.push(1);
    for _ in 0..400_000 {
        nested_arrays.extend_from_slice(&[117, 1]);
    }
    nested_arrays.extend_from_slice(&[126, 0]);
    let mut clients_with_no_pieces = vec![0xa0, 0x8d, 0x06];
    for client in 0..100_000_u32 {
        clients_with_no_pieces.push(0);
        let mut value = client;
        while value >= 0x80 {
            clients_with_no_pieces.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        clients_with_no_pieces.extend_from_slice(&[value as u8, 0]);
    }
    clients_with_no_pieces.push(0);
    let mut forger = Forger::new(&bob);

    for update in [nested_arrays, clients_with_no_pieces] {
        admin
            .receive(forger.seal(Op::DocUpdate {
                page: page.clone(),
                update,
            }))
            .unwrap();
    }
    assert_eq!(admin.page_text(&page).unwrap(), before);

    let state = admin.page_state(&page).unwrap();
    let honest = crate::pages::edit(state.as_deref(), crate::pages::Edit::Append("still editable")).unwrap();
    admin
        .receive(forger.seal(Op::DocUpdate {
            page: page.clone(),
            update: honest,
        }))
        .unwrap();
    assert!(admin.page_text(&page).unwrap().ends_with("still editable"));
}

#[tokio::test(flavor = "multi_thread")]
async fn page_updates_kept_before_the_page_rules_are_dropped_once() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.pages().unwrap()[0].id.clone();
    let state = admin.page_state(&page).unwrap();
    let mut with_trailing_bytes =
        crate::pages::edit(state.as_deref(), crate::pages::Edit::Append("PLANTED")).unwrap();
    with_trailing_bytes.push(0);
    let kept = Forger::new(&bob).seal(Op::DocUpdate {
        page: page.clone(),
        update: with_trailing_bytes,
    });
    admin.state().store.insert_applied(&kept, true).unwrap();
    admin.state().store.set_meta("views_version", &[4]).unwrap();

    let rebuilt = admin
        .state()
        .store
        .rebuild_if_rules_changed(admin.apply_context())
        .unwrap();

    assert!(rebuilt);
    assert!(!admin.page_text(&page).unwrap().contains("PLANTED"));
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
    let status = admin.status();
    assert!(status.needs_update, "the app says to update");
    assert_eq!(status.needs_update_from, [bob.my_id()], "and whose app sent it");
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
    assert!(status.needs_update_from.is_empty());
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

#[tokio::test(flavor = "multi_thread")]
async fn profile_details_sealed_in_a_dm_change_nothing() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let dm = bob.open_dm(&[admin.my_id()]).unwrap();
    bob.send_message(&dm, "hi", None).unwrap();
    sync_all(&[&bob, &admin]).await;
    let key = bob.state().store.channel_keys(Some(&dm)).unwrap().pop().unwrap();

    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal_private(
            &key,
            crate::private::ops::PrivateOp::Content(Op::ProfileDetails {
                details: titled("Smuggled"),
            }),
        ))
        .unwrap();
    assert_eq!(details_of(&admin, &bob.my_id()), Default::default());
    admin
        .receive(forger.seal_private(
            &key,
            crate::private::ops::PrivateOp::Content(Op::Message {
                id: crate::private::ids::new_private_message_id(&dm, &bob.me()),
                channel: dm.clone(),
                text: "still here".into(),
                reply_to: None,
            }),
        ))
        .unwrap();
    let texts: Vec<_> = admin
        .messages(&dm, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(texts.contains(&"still here".to_string()), "later ops still apply");
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_details_applied_just_before_a_removal_are_undone() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    bob.set_profile_details(&titled("Engineer")).unwrap();
    bob.sync().await;
    carol.sync().await;
    assert_eq!(details_of(&carol, &bob.my_id()).title, "Engineer");

    admin.remove_member(&bob.my_id()).await.unwrap();
    // Bob changes his details live, before Carol hears about the removal.
    carol
        .receive(Forger::new(&bob).seal(Op::ProfileDetails {
            details: titled("Admin"),
        }))
        .unwrap();
    assert_eq!(details_of(&carol, &bob.my_id()).title, "Admin");

    carol.sync().await;
    admin.sync().await;
    for member in [&admin, &carol] {
        assert_eq!(details_of(member, &bob.my_id()).title, "Engineer");
    }
}

/// A workspace whose recovery snapshot outgrows what restore reads still
/// changes membership: the snapshot is skipped, not the commit. The kit then
/// refuses to restore (its snapshot is stale) instead of splitting the group,
/// a new kit is refused with a clear error, and once the snapshot fits again
/// the next change uploads it.
#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_recovery_snapshot_does_not_hold_up_membership() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let store_sql = |sql: &str| admin.state().store.execute_for_test(sql);
    store_sql("INSERT INTO meta (key, value) VALUES ('padding', randomblob(34000000))");

    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "welcome, Carol", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert_eq!(admin.status().error, None);
    let settings = admin.storage_settings().unwrap();
    assert!(
        settings.recovery_snapshot_bytes.unwrap() > settings.recovery_snapshot_limit,
        "the app can tell the admin"
    );
    for member in [&bob, &carol] {
        assert!(texts_in(member, &general).contains(&"welcome, Carol".to_string()));
    }
    let stale = data_dir(&root.path().join("stale"));
    let restored = Workspace::restore_with(&stale, &kit, connector(&bucket), Networking::Offline).await;
    assert!(restored.is_err(), "the stale snapshot is refused");
    let refused = admin.recovery_kit().await.unwrap_err();
    assert!(format!("{refused:#}").contains("too large"), "{refused:#}");

    store_sql("DELETE FROM meta WHERE key = 'padding'");
    join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    drop(admin);
    let current = data_dir(&root.path().join("current"));
    let restored = Workspace::restore_with(&current, &kit, connector(&bucket), Networking::Offline).await;
    assert!(restored.is_ok(), "the next change uploaded a snapshot");
}

/// A removal in an admin's commit freezes the member's log at a sequence
/// number. One past anything the store holds must not stop members from
/// following commits: no op has such a number, so it freezes nothing more.
#[tokio::test(flavor = "multi_thread")]
async fn a_commit_freezing_a_log_past_any_stored_number_is_still_followed() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let from_epoch = carol.state().mls.epoch().unwrap();
    admin.remove_member(&bob.my_id()).await.unwrap();
    let signed = bucket.commit(from_epoch).await.unwrap().unwrap();
    let removal = commit::verify(&signed, from_epoch, &admin.me()).unwrap();
    let frozen_past_any_number = vec![FrozenAuthor {
        member: bob.me(),
        last_seq: u64::MAX,
    }];
    let resigned = commit::sign(
        &admin.inner.identity,
        from_epoch,
        removal.commit,
        frozen_past_any_number,
    )
    .unwrap();
    bucket.put_commit(from_epoch, resigned).await.unwrap();
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "after the removal", None).unwrap();
    // Not a full sync, which would put the original commit back.
    admin.push().await.unwrap();

    carol.sync().await;

    assert_eq!(carol.status().error, None);
    assert_eq!(
        carol.state().mls.epoch().unwrap(),
        admin.state().mls.epoch().unwrap()
    );
    assert!(texts_in(&carol, &general).contains(&"after the removal".to_string()));
}

/// Storage settings may change the region and keys, never the endpoint or
/// bucket (the data lives there), so a bad or hostile admin op can't point
/// members somewhere they then fail to open the workspace from.
#[tokio::test(flavor = "multi_thread")]
async fn storage_settings_cannot_move_the_workspace_or_empty_the_keys() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let current = admin.bucket_config().unwrap();
    let mut forger = Forger::new(&admin);
    let changed = |change: fn(&mut crate::transport::bucket::BucketConfig)| {
        let mut bucket = current.clone();
        change(&mut bucket);
        bucket
    };
    let refused = [
        changed(|bucket| bucket.endpoint = "http://198.51.100.7:9000".into()),
        changed(|bucket| bucket.endpoint = "not a url at all".into()),
        changed(|bucket| bucket.bucket = "somewhere-else".into()),
        changed(|bucket| bucket.access_key_id = String::new()),
        changed(|bucket| bucket.region = " ".into()),
    ];
    for bucket in refused {
        bob.receive(forger.seal(Op::StorageSettings { bucket })).unwrap();
        assert_eq!(bob.bucket_config().unwrap(), current);
    }

    let rotated = changed(|bucket| bucket.secret_access_key = "rotated-secret".into());
    bob.receive(forger.seal(Op::StorageSettings {
        bucket: rotated.clone(),
    }))
    .unwrap();
    assert_eq!(bob.bucket_config().unwrap(), rotated);
}

/// The rule on storage settings judges ops; it must not stop someone joining
/// a workspace whose creator left the region empty.
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_created_without_a_region_can_still_be_joined() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = Workspace::create_with(
        &data_dir(&root.path().join("ada")),
        "workspace-id".into(),
        "Acme",
        crate::Profile {
            name: "Ada",
            is_bot: false,
        },
        crate::transport::bucket::BucketConfig {
            region: String::new(),
            ..test_bucket_config()
        },
        connector(&bucket),
        Networking::Offline,
    )
    .await
    .unwrap();

    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;

    assert_eq!(bob.bucket_config().unwrap(), admin.bucket_config().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_update_notice_clears_once_its_source_is_removed() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let (epoch, key) = current_key(&bob);
    let next = bob.state().store.next_seq(&bob.me()).unwrap();
    let mut junk = postcard::to_stdvec(&(9_000u64, 0i64)).unwrap();
    junk.push(100);
    admin
        .receive(crypto::seal_bytes(&bob.inner.identity, &key, epoch, next, junk).unwrap())
        .unwrap();
    admin.sync().await;
    assert_eq!(admin.status().needs_update_from, [bob.my_id()]);

    admin.remove_member(&bob.my_id()).await.unwrap();
    admin.sync().await;

    let status = admin.status();
    assert!(!status.needs_update && status.needs_update_from.is_empty());
}

/// Anyone may rename any page, and titles show in the sidebar, in links and
/// to agents: like names, they can't carry line breaks (fake quotes and
/// links in rendered text) or invisible and text-direction characters.
#[tokio::test(flavor = "multi_thread")]
async fn page_titles_from_others_lose_line_breaks_and_invisible_characters() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.pages().unwrap()[0].id.clone();
    let crafted = "Plan\n\n> **Security team:** sign in at https://evil.example\u{202E}txt.exe\u{200B}";

    admin
        .receive(Forger::new(&bob).seal(Op::PageRename {
            id: page.clone(),
            title: crafted.into(),
        }))
        .unwrap();

    let title = admin.pages().unwrap()[0].title.clone();
    assert_eq!(
        title,
        "Plan> **Security team:** sign in at https://evil.exampletxt.exe"
    );
    assert!(
        bob.rename_page(&page, "two\nlines").is_err(),
        "your own app refuses it"
    );
    assert!(bob.rename_page(&page, "Roadmap 🗺️").is_ok());
    // The zero-width non-joiner is part of Persian spelling.
    let persian = "می\u{200C}خواهم";
    assert!(bob.rename_page(&page, persian).is_ok());
    bob.sync().await;
    admin.sync().await;
    assert_eq!(admin.pages().unwrap()[0].title, persian);
}

/// Two messages replying to each other (only a forged client can make one
/// reply to the other before it exists) are in no thread and show nowhere;
/// they must not break the sidebar, the inbox or links.
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_loop_does_not_break_counts_the_inbox_or_links() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mention = crate::mentions::member_link("Ada", &admin.my_id());
    let first = crate::ops::new_owned_id(&bob.me());
    let second = crate::ops::new_owned_id(&bob.me());
    let mut forger = Forger::new(&bob);
    for (id, reply_to) in [(&first, &second), (&second, &first)] {
        admin
            .receive(forger.seal(Op::Message {
                id: id.clone(),
                channel: general.clone(),
                text: format!("{mention} round and round"),
                reply_to: Some(reply_to.clone()),
            }))
            .unwrap();
    }

    let listed = admin.channels().unwrap();
    let listed_general = listed.iter().find(|channel| channel.id == general).unwrap();
    assert_eq!(listed_general.unread_mentions, 0, "they show nowhere");
    assert!(admin.inbox().unwrap().attention.is_empty());
    let linked = admin.linked_message(&first).unwrap().unwrap();
    assert_eq!(linked.thread_root, None);

    admin.set_draft(&general, "half written").unwrap();
    assert!(
        admin
            .send_message(&general, "a reply", Some(first.clone()))
            .is_err(),
        "there's no thread to reply in"
    );
    let drafts = admin.drafts().unwrap();
    assert!(
        drafts.iter().any(|draft| draft.text == "half written"),
        "the draft stays"
    );
}

/// A reply to a message in another channel isn't in that channel's thread,
/// so it shows in its own channel like any message, and reading the channel
/// reads it. Your own app doesn't send one.
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_to_another_channel_shows_where_it_was_sent() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let other = bob.create_channel("other").unwrap();
    let elsewhere = bob.send_message(&other, "over here", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let mention = crate::mentions::member_link("Ada", &admin.my_id());
    let reply = crate::ops::new_owned_id(&bob.me());
    admin
        .receive(Forger::new(&bob).seal(Op::Message {
            id: reply.clone(),
            channel: general.clone(),
            text: format!("{mention} across channels"),
            reply_to: Some(elsewhere.clone()),
        }))
        .unwrap();

    assert!(texts_in(&admin, &general).contains(&format!("{mention} across channels")));
    let unread_mentions = |admin: &Workspace| {
        admin
            .channels()
            .unwrap()
            .into_iter()
            .find(|channel| channel.id == general)
            .unwrap()
            .unread_mentions
    };
    assert_eq!(unread_mentions(&admin), 1);
    admin.mark_read(&general, &reply).unwrap();
    assert_eq!(unread_mentions(&admin), 0, "reading the channel reads it");
    // It starts a thread of its own here, and leaves the other channel's alone.
    let answer = admin
        .send_message(&general, "answering it", Some(reply.clone()))
        .unwrap();
    let thread: Vec<String> = admin.thread(&reply).unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(thread, [reply.clone(), answer]);
    assert_eq!(admin.thread(&elsewhere).unwrap().len(), 1);
    assert!(
        bob.send_message(&general, "nope", Some(elsewhere.clone()))
            .is_err()
    );
    let file = crate::workspace::NewFile {
        name: "nope.txt".into(),
        media_type: "text/plain".into(),
        source: crate::workspace::FileSource::Bytes(b"nope".to_vec()),
    };
    assert!(
        bob.send_files(&general, "", Some(elsewhere), vec![file])
            .await
            .is_err()
    );
}

/// Times come from their authors; arithmetic on extreme ones must not
/// overflow (a debug build would panic while holding the state lock).
#[tokio::test(flavor = "multi_thread")]
async fn extreme_times_from_others_cause_no_overflow() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let mut heartbeat = bob.heartbeat(PresenceState::Online).unwrap();
    for sent_ms in [i64::MIN, i64::MAX] {
        heartbeat.sent_ms = sent_ms;
        assert!(admin.receive_heartbeat(&heartbeat).is_err());
    }

    let general = channel_id(&admin, "general");
    let (epoch, key) = current_key(&bob);
    let seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let from_the_start_of_time = OpBody {
        lamport: 9_000,
        timestamp_ms: i64::MIN,
        op: Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: "very old news".into(),
            reply_to: None,
        },
    };
    admin
        .receive(crypto::seal(&bob.inner.identity, &key, epoch, seq, &from_the_start_of_time).unwrap())
        .unwrap();
    assert!(texts_in(&admin, &general).contains(&"very old news".to_string()));
}

/// A device whose stored bucket settings can't connect (say an older version
/// applied a bad admin op) says so plainly, and whom to ask, when it opens.
#[tokio::test(flavor = "multi_thread")]
async fn settings_that_cannot_connect_say_what_is_wrong_on_open() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let mut broken = bob.bucket_config().unwrap();
    broken.endpoint = "http://198.51.100.7:9000".into();
    bob.state()
        .store
        .set_meta("bucket_config", &serde_json::to_vec(&broken).unwrap())
        .unwrap();
    bob.shutdown().await.unwrap();
    drop(bob);
    let like_the_real_one: crate::transport::bucket::Connector = std::sync::Arc::new(
        move |_: &str, settings: &crate::transport::bucket::BucketConfig| {
            settings.endpoint_url()?;
            Ok(bucket.clone())
        },
    );

    let opened = Workspace::open_with(
        &data_dir(&root.path().join("bob")),
        like_the_real_one,
        Networking::Offline,
    )
    .await;

    let error = format!("{:#}", opened.err().unwrap());
    assert!(error.contains("ask the workspace admin"), "{error}");
    assert!(error.contains("https://"), "the reason stays: {error}");
}

/// workspace.json is a plain file; someone who can write to the data folder
/// mustn't make themselves admin on that device by editing it.
#[tokio::test(flavor = "multi_thread")]
async fn an_edited_workspace_json_is_refused() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let bob_dir = root.path().join("bob");
    bob.shutdown().await.unwrap();
    let bob_key = bob.me();
    drop(bob);
    let path = bob_dir.join("workspace.json");
    let mut settings: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    settings["admin"] = serde_json::to_value(bob_key).unwrap();
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();

    let opened = Workspace::open_with(&data_dir(&bob_dir), connector(&bucket), Networking::Offline).await;

    let error = format!("{:#}", opened.err().unwrap());
    assert!(error.contains("changed outside the app"), "{error}");
}

/// A delete can reach a device before the file it removes (rebuilds and
/// restores read the bucket in a new order, and gossip can race). The
/// deleter's device still takes the file's pieces out of the bucket.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_deleted_before_it_arrives_still_leaves_the_bucket() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let file = crate::workspace::NewFile {
        name: "notes.txt".into(),
        media_type: "text/plain".into(),
        source: crate::workspace::FileSource::Bytes(b"notes".to_vec()),
    };
    bob.send_files(&general, "", None, vec![file]).await.unwrap();
    let shared = bob.files().unwrap().remove(0).id;
    assert_eq!(bucket.blob_ids(&bob.me()).await.unwrap().len(), 1);

    admin.write(Op::Delete { id: shared }).unwrap();
    sync_all(&[&admin, &bob]).await;
    sync_all(&[&admin, &bob]).await;

    assert!(admin.files().unwrap().is_empty() && bob.files().unwrap().is_empty());
    assert!(bucket.blob_ids(&bob.me()).await.unwrap().is_empty());
}

/// An op's time comes from its author's clock. A message can't show a time
/// later than we got it, and lists keep the order ops were made in (their
/// clocks), not the times their authors claim.
#[tokio::test(flavor = "multi_thread")]
async fn claimed_times_cant_date_a_message_ahead_or_reorder_lists() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let (epoch, key) = current_key(&bob);
    let mut seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let a_year_ahead = now_ms().unwrap() + 365 * 24 * 60 * 60 * 1000;
    let mut seal = |lamport: u64, timestamp_ms: i64, op: Op| {
        let body = OpBody {
            lamport,
            timestamp_ms,
            op,
        };
        let envelope = crypto::seal(&bob.inner.identity, &key, epoch, seq, &body).unwrap();
        seq += 1;
        envelope
    };
    let made_first = crate::ops::new_owned_id(&bob.me());
    let made_second = crate::ops::new_owned_id(&bob.me());
    let ops = [
        seal(
            9_000,
            a_year_ahead,
            Op::ChannelCreate {
                id: made_first.clone(),
                name: "first".into(),
            },
        ),
        seal(
            9_001,
            1_000,
            Op::ChannelCreate {
                id: made_second.clone(),
                name: "second".into(),
            },
        ),
        seal(
            9_002,
            a_year_ahead,
            Op::Message {
                id: crate::ops::new_owned_id(&bob.me()),
                channel: general.clone(),
                text: "from the future".into(),
                reply_to: None,
            },
        ),
    ];
    for envelope in ops {
        admin.receive(envelope).unwrap();
    }

    let order: Vec<String> = admin
        .channels()
        .unwrap()
        .into_iter()
        .map(|channel| channel.id)
        .collect();
    let position = |id: &str| order.iter().position(|listed| listed == id).unwrap();
    assert!(position(&made_first) < position(&made_second), "{order:?}");
    let shown = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|message| message.text == "from the future")
        .unwrap();
    assert!(shown.created_ms <= now_ms().unwrap());
}
