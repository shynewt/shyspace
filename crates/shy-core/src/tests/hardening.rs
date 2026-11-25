//! Attacks from members, removed members and anyone with bucket access.
//! Every security rule in docs/security.md has a test here.

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_lite::{StreamExt, stream};
use iroh::SecretKey;
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory, path::Path,
};

use super::support::*;
use crate::{
    DataDir, KeyStorage, Networking, Workspace, crypto,
    membership::commit::{self, FrozenAuthor},
    ops::{Envelope, Op, OpBody, now_ms},
    transport::bucket::{Bucket, parse_segment_name, segment_name_for},
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
    let late_edit = page_update(
        &bob,
        &page,
        carol.page_state(&page).unwrap().as_deref(),
        crate::Edit::Append("SNEAKED-INTO-PAGE"),
    );
    for op in [
        Op::Message {
            id: last_word.clone(),
            channel: general.clone(),
            text: "SENT-AFTER-REMOVAL".into(),
            reply_to: None,
        },
        Op::Delete { id: earlier.clone() },
        late_edit,
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
        nonce: NONCE,
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
                nonce: NONCE,
                update,
            }))
            .unwrap();
    }
    assert_eq!(admin.page_text(&page).unwrap(), before);

    let state = admin.page_state(&page).unwrap();
    let honest = page_update(
        &bob,
        &page,
        state.as_deref(),
        crate::pages::Edit::Append("still editable"),
    );
    admin.receive(forger.seal(honest)).unwrap();
    assert!(admin.page_text(&page).unwrap().ends_with("still editable"));
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
    // An op body in a format this version doesn't know.
    let unknown_format = postcard::to_stdvec(&(crypto::OP_FORMAT + 1, "a new kind of change")).unwrap();
    let from_the_future = crypto::seal_bytes(&bob.inner.identity, &key, epoch, seq, unknown_format).unwrap();
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

/// Every op says which format it's in. One signed in our own format that
/// still doesn't decode is junk, not a newer version: it raises no update
/// notice, and it's recorded as handled so its author's later ops apply.
#[tokio::test(flavor = "multi_thread")]
async fn signed_junk_in_our_format_is_skipped_not_announced() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let (epoch, key) = current_key(&bob);
    let next = bob.state().store.next_seq(&bob.me()).unwrap();
    let junk = postcard::to_stdvec(&(crypto::OP_FORMAT, "not an op")).unwrap();
    let after = OpBody {
        lamport: 9_001,
        timestamp_ms: 0,
        op: Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: "after the junk".into(),
            reply_to: None,
        },
    };

    admin
        .receive(crypto::seal_bytes(&bob.inner.identity, &key, epoch, next, junk).unwrap())
        .unwrap();
    admin
        .receive(crypto::seal(&bob.inner.identity, &key, epoch, next + 1, &after).unwrap())
        .unwrap();
    admin.sync().await;

    let status = admin.status();
    assert!(!status.needs_update, "no update notice for junk");
    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(texts.contains(&"after the junk".to_string()), "{texts:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_op_far_ahead_raises_no_update_alarm() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let (epoch, key) = current_key(&bob);
    let far_ahead = bob.state().store.next_seq(&bob.me()).unwrap() + 500;
    let junk = postcard::to_stdvec(&(crypto::OP_FORMAT + 1, "a new kind of change")).unwrap();
    admin
        .receive(crypto::seal_bytes(&bob.inner.identity, &key, epoch, far_ahead, junk).unwrap())
        .unwrap();
    admin.sync().await;

    let status = admin.status();
    assert!(!status.needs_update);
    assert!(status.needs_update_from.is_empty());
    assert_eq!(status.error, None, "sync still reports success");
}

/// Anyone with the bucket keys can put objects among someone's segments: junk
/// that sorts after every real name, or a copy of their real ops under the
/// highest name. Neither may become where readers resume, or that person's
/// later segments would never be read.
#[tokio::test(flavor = "multi_thread")]
async fn objects_posing_as_later_segments_cannot_hide_someones_history() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "first", None).unwrap();
    let copied = bob.state().store.unpushed(&bob.me()).unwrap();
    sync_all(&[&bob, &admin]).await;
    let bob_segments = format!("ops/{}", bob.my_id());
    let highest = u64::MAX.to_string();
    bucket
        .put_raw(&format!("{bob_segments}/z"), b"junk".to_vec())
        .await
        .unwrap();
    bucket
        .put_raw(
            &format!("{bob_segments}/{highest}-{highest}"),
            postcard::to_stdvec(&copied).unwrap(),
        )
        .await
        .unwrap();
    admin.sync().await;

    bob.send_message(&general, "second", None).unwrap();
    bob.sync().await;
    admin.sync().await;

    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(texts.contains(&"second".to_string()), "{texts:?}");
}

/// Segment headers aren't signed: an author and seq in the clear prove
/// nothing until the op inside verifies. Forged ones under a matching name
/// must not move where reading resumes, whether they're dropped outright or
/// kept waiting for an epoch key.
#[tokio::test(flavor = "multi_thread")]
async fn forged_segment_headers_cannot_move_where_reading_resumes() {
    for is_kept_waiting in [false, true] {
        let (_root, bucket, admin, bob) = admin_and_bob().await;
        let general = channel_id(&admin, "general");
        let (epoch, _) = current_key(&bob);
        let next_seq = bob.state().store.next_seq(&bob.me()).unwrap();
        let (seq, epoch) = match is_kept_waiting {
            false => (u64::MAX, 0),
            true => (next_seq + 100, epoch + 1),
        };
        let forged = vec![Envelope {
            author: bob.me(),
            seq,
            epoch,
            nonce: [0; 24],
            ciphertext: vec![1, 2, 3],
        }];
        bucket
            .put_raw(
                &format!("ops/{}/{seq:020}-{seq:020}", bob.my_id()),
                postcard::to_stdvec(&forged).unwrap(),
            )
            .await
            .unwrap();
        admin.sync().await;

        bob.send_message(&general, "second", None).unwrap();
        bob.sync().await;
        admin.sync().await;

        let texts: Vec<_> = admin
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .map(|message| message.text)
            .collect();
        assert!(
            texts.contains(&"second".to_string()),
            "kept waiting: {is_kept_waiting}, {texts:?}"
        );
    }
}

/// Junk sorting after someone's last real segment can't become where reading
/// resumes, so it is listed every pass; it is fetched only once a run, so
/// it can't cost every reader its size on every pass.
#[tokio::test(flavor = "multi_thread")]
async fn junk_after_someones_last_segment_is_fetched_once_a_run() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;
    let junk_count = 20;
    for number in 0..junk_count {
        bucket
            .put_raw(&format!("ops/{}/z{number:04}", admin.my_id()), b"junk".to_vec())
            .await
            .unwrap();
    }
    bob.sync().await;

    let before = store.requests();
    bob.sync().await;
    let pass = store.requests().since(&before);

    assert!(pass.gets < junk_count, "{pass}");
}

/// A segment named with a hash of its bytes is read; one whose bytes don't
/// match that hash is ignored, ops and all.
#[tokio::test(flavor = "multi_thread")]
async fn a_segment_named_by_its_hash_must_match_it() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let texts = |admin: &Workspace| -> Vec<String> {
        admin
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .map(|message| message.text)
            .collect()
    };
    let named = |bytes: &[u8]| {
        let envelopes = bob.state().store.unpushed(&bob.me()).unwrap();
        let (first, last) = (envelopes.first().unwrap().seq, envelopes.last().unwrap().seq);
        crate::transport::bucket::segment_name_for(&bob.me(), first, last, &[5; 16], bytes)
    };

    bob.send_message(&general, "tampered", None).unwrap();
    let envelopes = bob.state().store.unpushed(&bob.me()).unwrap();
    let bytes = postcard::to_stdvec(&envelopes).unwrap();
    let tampered = named(b"other bytes");
    bucket.put_raw(&tampered, bytes.clone()).await.unwrap();
    admin.sync().await;
    assert!(!texts(&admin).contains(&"tampered".to_string()));

    bucket.delete_raw(&tampered).await.unwrap();
    bucket.put_raw(&named(&bytes), bytes).await.unwrap();
    admin.sync().await;
    assert!(texts(&admin).contains(&"tampered".to_string()));
}

/// A reader can be ahead of the bucket: gossip delivers ops before their
/// author uploads them. A copy of one of those, real or forged, put in the
/// bucket under its exact name must not become where reading resumes, or the
/// author's next upload (which sorts before it) would never be read.
#[tokio::test(flavor = "multi_thread")]
async fn a_copy_of_ops_received_live_cannot_hide_the_authors_next_upload() {
    for is_forged in [false, true] {
        let (_root, bucket, admin, bob) = admin_and_bob().await;
        let general = channel_id(&admin, "general");
        bob.send_message(&general, "first", None).unwrap();
        sync_all(&[&bob, &admin]).await;
        // Two ops reach admin live (gossip) before bob pushes them.
        bob.send_message(&general, "live one", None).unwrap();
        bob.send_message(&general, "live two", None).unwrap();
        let live = bob.state().store.unpushed(&bob.me()).unwrap();
        assert!(live.len() >= 2);
        for envelope in &live {
            admin.receive(envelope.clone()).unwrap();
        }
        // A member who also saw them live copies the last one to the bucket
        // under its exact name, before bob pushes.
        let mut last = live.last().unwrap().clone();
        if is_forged {
            last.ciphertext = vec![9; 8];
        }
        let seq = last.seq;
        bucket
            .put_raw(
                &format!("ops/{}/{seq:020}-{seq:020}", bob.my_id()),
                postcard::to_stdvec(&vec![last]).unwrap(),
            )
            .await
            .unwrap();
        admin.sync().await;

        // Admin goes quiet; bob writes more and pushes it all in one segment.
        bob.send_message(&general, "after", None).unwrap();
        bob.sync().await;
        admin.sync().await;

        let texts: Vec<_> = admin
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .map(|message| message.text)
            .collect();
        assert!(
            texts.contains(&"after".to_string()),
            "forged: {is_forged}, {texts:?}"
        );
    }
}

/// Anyone can guess an author's next segment names and put junk there first.
/// When the author's real upload replaces it, it is read, not skipped as the
/// junk that once stood there.
#[tokio::test(flavor = "multi_thread")]
async fn junk_at_the_authors_next_segment_name_cannot_hide_their_upload() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "first", None).unwrap();
    sync_all(&[&bob, &admin]).await;
    let next_seq = bob.state().store.next_seq(&bob.me()).unwrap();
    for last in next_seq..next_seq + 5 {
        bucket
            .put_raw(
                &format!("ops/{}/{next_seq:020}-{last:020}", bob.my_id()),
                b"junk".to_vec(),
            )
            .await
            .unwrap();
    }
    admin.sync().await;

    bob.send_message(&general, "hidden", None).unwrap();
    bob.sync().await;
    admin.sync().await;

    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(texts.contains(&"hidden".to_string()), "{texts:?}");
}

/// A copy of an op from inside a segment already read sorts after it under
/// an older name. It must not hold reading back: later uploads still arrive,
/// and nothing is stored to resume from.
#[tokio::test(flavor = "multi_thread")]
async fn a_copy_from_inside_a_read_segment_cannot_hold_reading_back() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    for text in ["one", "two", "three"] {
        bob.send_message(&general, text, None).unwrap();
    }
    let pushed = bob.state().store.unpushed(&bob.me()).unwrap();
    sync_all(&[&bob, &admin]).await;
    let middle = pushed[1].clone();
    let seq = middle.seq;
    bucket
        .put_raw(
            &format!("ops/{}/{seq:020}-{seq:020}", bob.my_id()),
            postcard::to_stdvec(&vec![middle]).unwrap(),
        )
        .await
        .unwrap();
    admin.sync().await;

    for text in ["later", "much later"] {
        bob.send_message(&general, text, None).unwrap();
        bob.sync().await;
        admin.sync().await;
    }

    let bob_last = bob.state().store.last_seq(&bob.me()).unwrap();
    assert_eq!(admin.state().store.last_seq(&bob.me()).unwrap(), bob_last);
}

/// A reader can hold an author's newest ops from gossip before their upload
/// lands. Single-op copies of those, uploaded first and read in an earlier
/// pass, must not hide the rest of the author's upload, even if the author
/// never writes again (#209).
#[tokio::test(flavor = "multi_thread")]
async fn copies_uploaded_before_the_authors_own_upload_cannot_hide_its_last_ops() {
    for has_copies in [false, true] {
        let (_root, bucket, admin, bob) = admin_and_bob().await;
        let general = channel_id(&admin, "general");
        bob.send_message(&general, "first", None).unwrap();
        sync_all(&[&bob, &admin]).await;
        for text in ["one", "two", "three"] {
            bob.send_message(&general, text, None).unwrap();
        }
        let unpushed = bob.state().store.unpushed(&bob.me()).unwrap();
        let (heard_live, missed) = unpushed.split_at(unpushed.len() - 1);
        for envelope in heard_live {
            admin.receive(envelope.clone()).unwrap();
        }
        if has_copies {
            for envelope in heard_live {
                let seq = envelope.seq;
                bucket
                    .put_raw(
                        &format!("ops/{}/{seq:020}-{seq:020}", bob.my_id()),
                        postcard::to_stdvec(&vec![envelope.clone()]).unwrap(),
                    )
                    .await
                    .unwrap();
            }
            admin.sync().await;
        }

        bob.sync().await;
        admin.sync().await;

        assert!(
            admin.state().store.is_applied(&bob.me(), missed[0].seq).unwrap(),
            "copies: {has_copies}"
        );
    }
}

/// Removing someone freezes their log at what they really wrote, even when
/// copies of ops the admin heard live were uploaded ahead of theirs.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_freezes_at_the_true_last_op_despite_copies_uploaded_ahead() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    for text in ["one", "two", "three"] {
        bob.send_message(&general, text, None).unwrap();
    }
    let unpushed = bob.state().store.unpushed(&bob.me()).unwrap();
    let (heard_live, _) = unpushed.split_at(unpushed.len() - 1);
    for envelope in heard_live {
        admin.receive(envelope.clone()).unwrap();
        let seq = envelope.seq;
        bucket
            .put_raw(
                &format!("ops/{}/{seq:020}-{seq:020}", bob.my_id()),
                postcard::to_stdvec(&vec![envelope.clone()]).unwrap(),
            )
            .await
            .unwrap();
    }
    admin.sync().await;
    bob.sync().await;

    admin.remove_member(&bob.my_id()).await.unwrap();

    let bob_last = bob.state().store.last_seq(&bob.me()).unwrap();
    assert_eq!(
        admin.state().store.frozen_last_seq(&bob.me()).unwrap(),
        Some(bob_last)
    );
}

/// A segment that couldn't become the resume point is skipped for the rest
/// of the run, but gap repair still reads it again when its ops are lost.
#[tokio::test(flavor = "multi_thread")]
async fn gap_repair_reads_a_skipped_segment_again() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "second", None).unwrap();
    bob.send_message(&general, "third", None).unwrap();
    bob.sync().await;
    admin
        .inner
        .fail_next_apply
        .store(true, std::sync::atomic::Ordering::SeqCst);
    admin.sync().await;
    assert!(!admin.inner.skipped_segments.lock().unwrap().is_empty());
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
    admin.sync().await;
    admin.sync().await;

    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    for text in ["second", "third", "fourth"] {
        assert!(texts.contains(&text.to_string()), "{text}: {texts:?}");
    }
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

/// Channel names from anyone follow one rule, created or renamed: a creation
/// whose name breaks it (control or invisible characters, punctuation the
/// rule doesn't allow) is skipped, the same on every device.
#[tokio::test(flavor = "multi_thread")]
async fn channels_created_with_names_that_break_the_rule_are_skipped() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let (epoch, key) = current_key(&bob);
    let first_seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let names = ["q&a", "\u{202e}evil\u{200b}name", "plain-name"];
    for (offset, name) in names.into_iter().enumerate() {
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

    let shown: Vec<_> = admin
        .channels()
        .unwrap()
        .into_iter()
        .map(|channel| channel.name)
        .collect();
    assert!(shown.contains(&"plain-name".to_string()), "{shown:?}");
    assert!(
        !shown
            .iter()
            .any(|name| name.contains("q&a") || name.contains("evil")),
        "{shown:?}"
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
    // Shown as a plain message, like a reply whose parent is missing.
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
        TEST_WORKSPACE_ID.into(),
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
    let newer = postcard::to_stdvec(&(crypto::OP_FORMAT + 1, "a new kind of change")).unwrap();
    admin
        .receive(crypto::seal_bytes(&bob.inner.identity, &key, epoch, next, newer).unwrap())
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

/// A device whose stored bucket settings can't connect says so plainly, and
/// whom to ask, when it opens.
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
    let page = admin.pages().unwrap()[0].id.clone();
    let state = admin.page_state(&page).unwrap();
    let page_edit = page_update(&bob, &page, state.as_deref(), crate::pages::Edit::Append("later"));
    let ops = [
        seal(9_003, a_year_ahead, page_edit),
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

    // The same after the views are built again from the log.
    admin.state().store.rebuild_views(admin.apply_context()).unwrap();
    let edited = admin
        .pages()
        .unwrap()
        .into_iter()
        .find(|listed| listed.id == page)
        .unwrap();
    assert!(
        edited.updated_ms <= now_ms().unwrap(),
        "the page edit isn't dated ahead"
    );
}

/// Hints are signed by their author and name a head version. A replayed old
/// hint, or one not signed by its author, can't make a reader skip a head
/// that moved.
#[tokio::test(flavor = "multi_thread")]
async fn false_or_replayed_hints_cannot_stop_a_head_being_read() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let bob_head_version = || async {
        let heads = bucket.heads().await.unwrap();
        heads
            .into_iter()
            .find(|head| head.author == bob.me())
            .unwrap()
            .version
    };
    bob.send_message(&general, "one", None).unwrap();
    for envelope in bob.state().store.unpushed(&bob.me()).unwrap() {
        admin.receive(envelope).unwrap();
    }
    bob.sync().await;
    let old_seq = bob.state().store.last_seq(&bob.me()).unwrap();
    let old_hint = bob.signed_hint(old_seq, bob_head_version().await).unwrap();
    admin.receive_hint(&old_hint).unwrap();
    admin.check().await;

    bob.send_message(&general, "two", None).unwrap();
    bob.sync().await;
    let new_seq = bob.state().store.last_seq(&bob.me()).unwrap();
    let mut not_bobs = admin.signed_hint(new_seq, bob_head_version().await).unwrap();
    not_bobs.author = bob.me();
    assert!(admin.receive_hint(&not_bobs).is_err(), "not signed by Bob");
    admin.receive_hint(&old_hint).unwrap();

    admin.check().await;

    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(texts.contains(&"two".to_string()), "{texts:?}");
}

/// Anyone with the bucket keys can write someone's head. One claiming more
/// than its author wrote is corrected by them at their next check, so
/// readers don't stop seeing their uploads move it.
#[tokio::test(flavor = "multi_thread")]
async fn a_falsely_high_head_is_corrected_by_its_author() {
    let (_root, bucket, _admin, bob) = admin_and_bob().await;
    let general = channel_id(&_admin, "general");
    bob.send_message(&general, "one", None).unwrap();
    bob.sync().await;
    let high = crate::transport::heads::Head {
        last_seq: 1_000_000,
        key: [0; 8],
    };
    bucket.put_head(&bob.me(), high).await.unwrap();

    bob.check().await;

    let pushed = bob.state().store.last_pushed_seq(&bob.me()).unwrap();
    let head = bucket.head(&bob.me()).await.unwrap().unwrap();
    assert_eq!(head.last_seq, pushed);
}

/// Heads under keys that were never members are ignored: anyone with the
/// bucket keys could otherwise make every reader fetch junk heads.
#[tokio::test(flavor = "multi_thread")]
async fn heads_of_strangers_are_not_read() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;
    let strangers = 20;
    for _ in 0..strangers {
        let stranger = SecretKey::generate().public();
        bucket
            .put_head(
                &stranger,
                crate::transport::heads::Head {
                    last_seq: 5,
                    key: [0; 8],
                },
            )
            .await
            .unwrap();
    }

    let before = store.requests();
    bob.check().await;

    assert!(
        store.requests().since(&before).gets < strangers,
        "{}",
        store.requests().since(&before)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn members_cannot_write_under_another_editors_client_id() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    let page = admin.pages().unwrap()[0].id.clone();
    let state = admin.page_state(&page).unwrap();

    // Bob's editor writes " good"; Carol writes " EVIL" at the same place
    // under Bob's client id, with a nonce of her own and with Bob's.
    let session = bob.page_editor_session(&page);
    let bobs_nonce: [u8; 16] = hex::decode(&session.nonce).unwrap().try_into().unwrap();
    let edit = |text: &str| {
        crate::pages::edit(state.as_deref(), crate::Edit::Append(text), session.client_id).unwrap()
    };
    let good = Forger::new(&bob).seal(Op::DocUpdate {
        page: page.clone(),
        nonce: bobs_nonce,
        update: edit(" good"),
    });
    let mut forger = Forger::new(&carol);
    let with_her_nonce = forger.seal(Op::DocUpdate {
        page: page.clone(),
        nonce: [9; 16],
        update: edit(" EVIL"),
    });
    let with_his_nonce = forger.seal(Op::DocUpdate {
        page: page.clone(),
        nonce: bobs_nonce,
        update: edit(" EVIL"),
    });

    for envelope in [&with_her_nonce, &with_his_nonce, &good] {
        admin.receive(envelope.clone()).unwrap();
    }
    for envelope in [&good, &with_her_nonce, &with_his_nonce] {
        dave.receive(envelope.clone()).unwrap();
    }
    let text = admin.page_text(&page).unwrap();
    assert!(text.ends_with(" good"), "{text}");
    assert!(!text.contains("EVIL"));
    assert_eq!(dave.page_text(&page).unwrap(), text);

    // A rebuild judges them the same way, even kept whole in the log.
    for envelope in [&with_her_nonce, &with_his_nonce] {
        admin.state().store.insert_applied(envelope, true).unwrap();
    }
    admin.state().store.rebuild_views(admin.apply_context()).unwrap();
    assert_eq!(admin.page_text(&page).unwrap(), text);
}

/// The hourly sweep is timed by the wall clock and saved: due once the hour
/// is up, and at once if the clock was set back (the saved time is then
/// ahead of now) rather than put off.
#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_is_due_hourly_and_at_once_after_the_clock_is_set_back() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let hour_ms = 60 * 60 * 1000;
    let now = now_ms().unwrap();
    admin.set_last_sweep(now);
    assert!(!admin.is_sweep_due().unwrap());
    admin.set_last_sweep(now - hour_ms);
    assert!(admin.is_sweep_due().unwrap());
    admin.set_last_sweep(now + 24 * hour_ms);
    assert!(admin.is_sweep_due().unwrap(), "the clock was set back");
}

/// A sweep that stopped on our own failure (a full disk) isn't counted as
/// done, so the next pass sweeps again instead of waiting an hour.
#[tokio::test(flavor = "multi_thread")]
async fn a_sweep_cut_short_by_a_local_failure_is_still_due() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "one", None).unwrap();
    bob.sync().await;
    admin.set_last_sweep(0);
    for hook in [&admin.inner.fail_next_apply, &admin.inner.fail_next_park] {
        hook.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    admin.sync().await;

    assert!(admin.status().error.is_some());
    assert!(admin.is_sweep_due().unwrap());
}

/// Like S3: an object's ETag is a hash of its content, so putting the same
/// bytes again gives the same ETag. Can swap an object right before a GET.
#[derive(Debug, Default)]
struct ContentTagStore {
    inner: InMemory,
    tags: Arc<Mutex<HashMap<Path, String>>>,
    swap_before_get: Mutex<Option<(Path, Bytes)>>,
}

fn tag_of(payload: &PutPayload) -> String {
    let mut hasher = blake3::Hasher::new();
    for chunk in payload.iter() {
        hasher.update(chunk);
    }
    hasher.finalize().to_hex().to_string()
}

impl fmt::Display for ContentTagStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("content tag store")
    }
}

#[async_trait]
impl ObjectStore for ContentTagStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.tags
            .lock()
            .unwrap()
            .insert(location.clone(), tag_of(&payload));
        self.inner.put_opts(location, payload, opts).await
    }
    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, opts).await
    }
    async fn get_opts(&self, location: &Path, options: GetOptions) -> object_store::Result<GetResult> {
        let swap = {
            let mut armed = self.swap_before_get.lock().unwrap();
            match armed.as_ref() {
                Some((path, _)) if path == location => armed.take(),
                _ => None,
            }
        };
        if let Some((path, bytes)) = swap {
            self.put_opts(&path, PutPayload::from(bytes), PutOptions::default())
                .await?;
        }
        let mut result = self.inner.get_opts(location, options).await?;
        result.meta.e_tag = self.tags.lock().unwrap().get(location).cloned();
        Ok(result)
    }
    fn delete_stream(
        &self,
        locations: stream::Boxed<object_store::Result<Path>>,
    ) -> stream::Boxed<object_store::Result<Path>> {
        self.inner.delete_stream(locations)
    }
    fn list(&self, prefix: Option<&Path>) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        let tags = self.tags.clone();
        Box::pin(self.inner.list(prefix).map(move |meta| {
            meta.map(|mut meta| {
                meta.e_tag = tags.lock().unwrap().get(&meta.location).cloned();
                meta
            })
        }))
    }
    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        let tags = self.tags.clone();
        Box::pin(self.inner.list_with_offset(prefix, offset).map(move |meta| {
            meta.map(|mut meta| {
                meta.e_tag = tags.lock().unwrap().get(&meta.location).cloned();
                meta
            })
        }))
    }
    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }
    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> object_store::Result<()> {
        self.inner.copy_opts(from, to, options).await
    }
}

/// A commit slot is verified at the version the bytes were read at, not the
/// one listed: otherwise someone swapping it between junk and the real
/// commit around our listing could make us skip the junk for good (S3
/// ETags are content hashes, so the junk's ETag repeats).
#[tokio::test(flavor = "multi_thread")]
async fn a_commit_swapped_around_a_listing_is_still_put_back() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(ContentTagStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let (epoch, signed) = bob
        .state()
        .store
        .commit_records()
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let junk = b"junk".to_vec();

    bucket.put_commit(epoch, junk.clone()).await.unwrap();
    *store.swap_before_get.lock().unwrap() = Some((
        Path::from(format!("mls/commit/{epoch:020}")),
        Bytes::from(signed.clone()),
    ));
    bob.make_slow_duties_due();
    bob.check().await;
    assert!(
        store.swap_before_get.lock().unwrap().is_none(),
        "the swap happened"
    );

    bucket.put_commit(epoch, junk).await.unwrap();
    bob.make_slow_duties_due();
    bob.check().await;

    assert_eq!(
        bucket.commit(epoch).await.unwrap().as_deref(),
        Some(signed.as_slice()),
        "bob should repair the commit"
    );
}

/// One of Bob's segments goes missing, so the admin's checked mark for him
/// can't pass it. Junk planted over a later range the admin holds is still
/// fetched at most once a run, not at every sweep.
#[tokio::test(flavor = "multi_thread")]
async fn junk_over_a_held_range_is_fetched_once_a_run() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;
    let general = channel_id(&admin, "general");

    bob.send_message(&general, "one", None).unwrap();
    let first_batch = bob.state().store.unpushed(&bob.me()).unwrap();
    for envelope in first_batch.clone() {
        admin.receive(envelope).unwrap();
    }
    bob.sync().await;
    let listed = bucket.list_segments(&bob.me(), None).await.unwrap();
    let missing = listed
        .iter()
        .find(|object| parse_segment_name(&bob.me(), &object.key).unwrap().first == first_batch[0].seq)
        .unwrap();
    bucket.delete_raw(&missing.key).await.unwrap();

    bob.send_message(&general, "two", None).unwrap();
    let second_batch = bob.state().store.unpushed(&bob.me()).unwrap();
    for envelope in second_batch.clone() {
        admin.receive(envelope).unwrap();
    }
    bob.sync().await;
    admin.sync().await;
    let before = store.requests();
    admin.sync().await;
    let idle_sweep = store.requests().since(&before).gets;

    let last = second_batch.last().unwrap();
    let junk = Envelope {
        ciphertext: vec![7; 64],
        ..last.clone()
    };
    bucket.put_segment(&[junk]).await.unwrap();
    admin.sync().await;
    let before = store.requests();
    admin.sync().await;

    assert_eq!(
        store.requests().since(&before).gets,
        idle_sweep,
        "junk is fetched once per run at most"
    );
}

/// Anyone with the epoch key can encrypt an honest op again and gossip that
/// copy. It's still the op its author signed, so the author's own upload
/// still counts as checked against the bucket, and their mark moves on.
#[tokio::test(flavor = "multi_thread")]
async fn a_live_copy_encrypted_again_does_not_stop_the_checked_mark() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let (_, epoch_key) = current_key(&admin);
    bob.send_message(&general, "one", None).unwrap();
    for envelope in bob.state().store.unpushed(&bob.me()).unwrap() {
        admin.receive(encrypted_again(&envelope, &epoch_key, 0)).unwrap();
    }
    bob.sync().await;
    admin.sync().await;
    bob.send_message(&general, "two", None).unwrap();
    bob.sync().await;
    admin.sync().await;
    admin.sync().await;

    let held = admin.state().store.last_seq(&bob.me()).unwrap();
    assert_eq!(admin.checked_against_bucket(&bob.me()).unwrap(), held);
}

/// The admin repairs commits only once its recovery snapshot is up: a
/// commit must never reach the bucket before the snapshot that covers it.
#[tokio::test(flavor = "multi_thread")]
async fn the_admin_repairs_commits_only_after_its_snapshot_is_up() {
    let (_root, bucket, admin, _bob) = admin_and_bob().await;
    admin.recovery_kit().await.unwrap();
    let epoch = admin.state().store.commit_records().unwrap()[0].0;
    let key = admin.state().store.meta("recovery_key").unwrap().unwrap();
    let corrupt = |admin: &Workspace| {
        let store = &admin.state().store;
        store.set_meta("recovery_key", &[1, 2, 3]).unwrap();
        store.set_meta("recovery_uploaded_generation", &[0; 8]).unwrap();
        store.set_meta("recovery_change_generation", &[9; 8]).unwrap();
    };
    corrupt(&admin);
    bucket.delete_commit(epoch).await.unwrap();

    admin.make_slow_duties_due();
    admin.sync().await;
    assert!(
        bucket.commit(epoch).await.unwrap().is_none(),
        "not before the snapshot"
    );

    admin.state().store.set_meta("recovery_key", &key).unwrap();
    admin.make_slow_duties_due();
    admin.sync().await;
    assert!(bucket.commit(epoch).await.unwrap().is_some(), "after it");
}

/// The workspace's id and admin are kept in the encrypted store from the
/// start; a store without them doesn't open, rather than taking whatever
/// the plain settings file says.
#[tokio::test(flavor = "multi_thread")]
async fn a_store_missing_its_identity_does_not_open() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let dir = root.path().join("ada");
    let admin = create_admin(&dir, &bucket).await;
    admin
        .state()
        .store
        .execute_for_test("DELETE FROM meta WHERE key = 'workspace_identity'");
    admin.shutdown().await.unwrap();
    drop(admin);

    let opened = Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline).await;

    let error = format!("{:#}", opened.err().expect("refused"));
    assert!(error.contains("id and admin are missing"), "{error}");
}

/// A store whose next listing under a prefix fails, as a network blip
/// would, and which can refuse every upload to keys starting with one. It
/// records the prefix of every listing. Objects can also be made unreadable,
/// as one put with a customer key (SSE-C) or in an archive storage class is
/// to a plain GET, until something is written over them.
#[derive(Debug, Default)]
struct FlakyStore {
    inner: InMemory,
    fail_next_list_under: Mutex<Option<String>>,
    refuse_puts_under: Mutex<Option<String>>,
    listed_prefixes: Mutex<Vec<String>>,
    unreadable: Mutex<std::collections::HashSet<String>>,
    /// Keys whose next download fails, as a network blip would.
    unreadable_once: Mutex<std::collections::HashSet<String>>,
    refused_reads: std::sync::atomic::AtomicUsize,
    /// Every download claims a size of one byte, whatever it holds.
    understates_sizes: std::sync::atomic::AtomicBool,
}

impl fmt::Display for FlakyStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("flaky store")
    }
}

impl FlakyStore {
    fn note_listing(&self, prefix: Option<&Path>) {
        let prefix = prefix.map(Path::to_string).unwrap_or_default();
        self.listed_prefixes.lock().unwrap().push(prefix);
    }

    fn listings_of(&self, prefix: &str) -> usize {
        let listed = self.listed_prefixes.lock().unwrap();
        listed.iter().filter(|listed| listed.as_str() == prefix).count()
    }

    fn is_failing(&self, prefix: Option<&Path>) -> bool {
        let mut armed = self.fail_next_list_under.lock().unwrap();
        let is_hit = matches!((armed.as_deref(), prefix), (Some(armed), Some(prefix)) if prefix.as_ref().starts_with(armed));
        if is_hit {
            *armed = None;
        }
        is_hit
    }

    fn failure() -> object_store::Error {
        object_store::Error::Generic {
            store: "flaky",
            source: "a network blip".into(),
        }
    }

    /// Puts `bytes` at `key` and makes it unreadable, as a member with the
    /// bucket keys could.
    async fn plant_unreadable(&self, key: &str, bytes: &'static [u8]) {
        self.inner
            .put_opts(
                &Path::from(key),
                PutPayload::from_static(bytes),
                PutOptions::default(),
            )
            .await
            .unwrap();
        self.unreadable.lock().unwrap().insert(key.to_owned());
    }

    async fn keys_under(&self, prefix: &str) -> Vec<String> {
        let mut keys: Vec<String> = self
            .inner
            .list(Some(&Path::from(prefix)))
            .map(|meta| meta.unwrap().location.to_string())
            .collect()
            .await;
        keys.sort();
        keys
    }
}

#[async_trait]
impl ObjectStore for FlakyStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        let is_refused = self
            .refuse_puts_under
            .lock()
            .unwrap()
            .as_deref()
            .is_some_and(|refused| location.as_ref().starts_with(refused));
        if is_refused {
            return Err(Self::failure());
        }
        let put = self.inner.put_opts(location, payload, opts).await?;
        self.unreadable.lock().unwrap().remove(location.as_ref());
        Ok(put)
    }
    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, opts).await
    }
    async fn get_opts(&self, location: &Path, options: GetOptions) -> object_store::Result<GetResult> {
        let is_blip = self.unreadable_once.lock().unwrap().remove(location.as_ref());
        if is_blip || self.unreadable.lock().unwrap().contains(location.as_ref()) {
            self.refused_reads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(object_store::Error::PermissionDenied {
                path: location.to_string(),
                source: "the object is archived".into(),
            });
        }
        let mut result = self.inner.get_opts(location, options).await?;
        if self.understates_sizes.load(std::sync::atomic::Ordering::SeqCst) {
            result.meta.size = 1;
        }
        Ok(result)
    }
    fn delete_stream(
        &self,
        locations: stream::Boxed<object_store::Result<Path>>,
    ) -> stream::Boxed<object_store::Result<Path>> {
        self.inner.delete_stream(locations)
    }
    fn list(&self, prefix: Option<&Path>) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        self.note_listing(prefix);
        match self.is_failing(prefix) {
            true => Box::pin(stream::once(Err(Self::failure()))),
            false => self.inner.list(prefix),
        }
    }
    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        self.note_listing(prefix);
        match self.is_failing(prefix) {
            true => Box::pin(stream::once(Err(Self::failure()))),
            false => self.inner.list_with_offset(prefix, offset),
        }
    }
    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }
    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> object_store::Result<()> {
        self.inner.copy_opts(from, to, options).await
    }
}

/// Ada, with Bob and Carol, on a store whose objects can be made unreadable.
async fn on_a_flaky_store(
    root: &std::path::Path,
) -> (Arc<FlakyStore>, Bucket, Workspace, Workspace, Workspace) {
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.join("ada"), &bucket).await;
    let bob = join(&admin, &root.join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    (store, bucket, admin, bob, carol)
}

/// `author`'s newest segment in the bucket.
async fn newest_segment(store: &FlakyStore, author: &Workspace) -> String {
    store
        .keys_under(&format!("ops/{}/", author.my_id()))
        .await
        .pop()
        .unwrap()
}

/// An object a member with the keys made unreadable, named to be fetched
/// before the real segments and to claim far more ops than were written,
/// doesn't stop the rest of the log being read. It counts as a read that
/// didn't finish until it has failed a few times, then it is given up for
/// the run like junk, so it can't cost a download on every pass.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_object_among_segments_doesnt_stop_reading() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, _carol) = on_a_flaky_store(root.path()).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "landed", None).unwrap();
    bob.sync().await;
    plant_far_ahead(&store, &bob).await;

    admin.sync().await;
    assert!(texts_in(&admin, &general).contains(&"landed".to_owned()));
    assert!(admin.status().error.is_some(), "the read didn't finish");
    for _ in 0..2 {
        admin.sync().await;
    }

    assert_eq!(admin.status().error, None, "given up at the third try");
}

/// Plants an unreadable object among `author`'s segments, named to claim
/// far more ops than they wrote.
async fn plant_far_ahead(store: &FlakyStore, author: &Workspace) {
    let real = newest_segment(store, author).await;
    let (prefix, name) = real.rsplit_once('/').unwrap();
    let fields: Vec<&str> = name.split('-').collect();
    let last: u64 = fields[0].parse().unwrap();
    let planted = format!(
        "{prefix}/{:0width$}-{:0first_width$}-{}-{}",
        last + 1000,
        1,
        "0".repeat(fields[2].len()),
        fields[3],
        width = fields[0].len(),
        first_width = fields[1].len(),
    );
    store.plant_unreadable(&planted, b"junk").await;
}

/// An object given up stays given up while its author has a gap: gap
/// repair reads their log again, but doesn't download it again on every
/// check or keep the error showing.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_object_stays_given_up_through_gap_repair() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, _carol) = on_a_flaky_store(root.path()).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "second", None).unwrap();
    bob.sync().await;
    let second = newest_segment(&store, &bob).await;
    plant_far_ahead(&store, &bob).await;
    admin
        .inner
        .fail_next_apply
        .store(true, std::sync::atomic::Ordering::SeqCst);
    admin.check().await;
    // As if the parked op was pushed out by the cap, and gone from the
    // bucket: a gap that can't be filled.
    admin.state().store.execute_for_test("DELETE FROM pending_ops");
    let removed: Vec<_> = store
        .inner
        .delete_stream(Box::pin(stream::once(Ok(Path::from(second)))))
        .collect()
        .await;
    assert!(removed.iter().all(Result::is_ok));
    bob.send_message(&general, "third", None).unwrap();
    let third = bob
        .state()
        .store
        .unpushed(&bob.me())
        .unwrap()
        .last()
        .unwrap()
        .clone();
    admin.receive(third).unwrap();
    assert!(
        !admin.state().store.authors_missing_next_op().unwrap().is_empty(),
        "a gap formed"
    );

    for _ in 0..8 {
        admin.check().await;
    }

    let downloads = store.refused_reads.load(std::sync::atomic::Ordering::SeqCst);
    assert!(downloads <= 3, "downloaded {downloads} times");
    assert_eq!(admin.status().error, None);
}

/// A check that couldn't download a segment doesn't count the author's
/// head as read, so the next check reads it once it can be downloaded.
#[tokio::test(flavor = "multi_thread")]
async fn a_segment_that_couldnt_be_fetched_is_read_at_the_next_check() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, _carol) = on_a_flaky_store(root.path()).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "landed", None).unwrap();
    bob.sync().await;
    let real = newest_segment(&store, &bob).await;
    store.unreadable.lock().unwrap().insert(real.clone());
    admin.check().await;
    assert!(!texts_in(&admin, &general).contains(&"landed".to_owned()));
    assert!(admin.status().error.is_some(), "the read didn't finish");

    store.unreadable.lock().unwrap().remove(&real);
    admin.check().await;

    assert!(texts_in(&admin, &general).contains(&"landed".to_owned()));
}

/// A sweep that couldn't download a segment isn't marked done.
#[tokio::test(flavor = "multi_thread")]
async fn a_sweep_that_couldnt_download_a_segment_stays_due() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, _carol) = on_a_flaky_store(root.path()).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "landed", None).unwrap();
    bob.sync().await;
    let real = newest_segment(&store, &bob).await;
    store.unreadable.lock().unwrap().insert(real);
    // As if the last sweep was long ago.
    admin
        .state()
        .store
        .execute_for_test("DELETE FROM meta WHERE key = 'last_sweep_ms'");

    admin.sync().await;

    assert!(admin.is_sweep_due().unwrap());
}

/// A restore that can't download all of the admin's log is refused, rather
/// than letting the restored admin reuse seqs the bucket already holds,
/// which every device would prove a fork.
#[tokio::test(flavor = "multi_thread")]
async fn a_restore_that_cant_download_the_admins_log_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket, admin, _bob, _carol) = on_a_flaky_store(root.path()).await;
    let general = channel_id(&admin, "general");
    let kit = admin.recovery_kit().await.unwrap();
    admin.send_message(&general, "after the kit", None).unwrap();
    admin.sync().await;
    let real = newest_segment(&store, &admin).await;
    drop(admin);
    store.unreadable.lock().unwrap().insert(real);

    let restored = Workspace::restore_with(
        &data_dir(&root.path().join("ada-new")),
        &kit,
        connector(&bucket),
        Networking::Offline,
    )
    .await;

    let error = format!("{:#}", restored.err().expect("refused"));
    assert!(error.contains("couldn't catch up"), "{error}");
}

/// A member being removed can plant a fresh unreadable object among their
/// segments before each attempt, so it is never given up. The removal goes
/// on anyway, freezing their log at what could be read.
#[tokio::test(flavor = "multi_thread")]
async fn a_member_cant_block_their_removal_with_unreadable_objects() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, _carol) = on_a_flaky_store(root.path()).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "last words", None).unwrap();
    bob.sync().await;
    plant_far_ahead(&store, &bob).await;

    admin.remove_member(&bob.my_id()).await.unwrap();

    assert!(!admin.current_members().unwrap().contains(&bob.me()));
    assert!(texts_in(&admin, &general).contains(&"last words".to_owned()));
}

/// A network blip on the member's last segment during their removal is
/// ridden out: the removal doesn't freeze their log short of messages
/// others already have, which every device would then undo.
#[tokio::test(flavor = "multi_thread")]
async fn a_blip_during_a_removal_keeps_the_members_last_messages() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, carol) = on_a_flaky_store(root.path()).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "last words", None).unwrap();
    bob.sync().await;
    carol.sync().await;
    let last = newest_segment(&store, &bob).await;
    store.unreadable_once.lock().unwrap().insert(last);

    admin.remove_member(&bob.my_id()).await.unwrap();
    carol.sync().await;

    assert!(texts_in(&admin, &general).contains(&"last words".to_owned()));
    assert!(texts_in(&carol, &general).contains(&"last words".to_owned()));
}

/// A segment of the member's given up earlier in the run gets fresh tries
/// when they are removed, rather than being left out without a word.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_tries_again_what_was_given_up_earlier() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, _carol) = on_a_flaky_store(root.path()).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "last words", None).unwrap();
    bob.sync().await;
    let last = newest_segment(&store, &bob).await;
    store.unreadable.lock().unwrap().insert(last.clone());
    for _ in 0..3 {
        admin.check().await;
    }
    store.unreadable.lock().unwrap().remove(&last);

    admin.remove_member(&bob.my_id()).await.unwrap();

    assert!(texts_in(&admin, &general).contains(&"last words".to_owned()));
}

/// Two versions of Bob's next op, proved a fork on the admin's device and
/// shared to the bucket. Returns the seq and the proof's path.
async fn a_shared_fork(admin: &Workspace, bob: &Workspace, store: &FlakyStore) -> (u64, String) {
    let general = channel_id(admin, "general");
    let message = |text: &str| crate::ops::Op::Message {
        id: crate::ops::new_owned_id(&bob.me()),
        channel: general.clone(),
        text: text.into(),
        reply_to: None,
    };
    let one = Forger::new(bob).seal(message("one version"));
    let other = Forger::new(bob).seal(message("another version"));
    admin.receive(one.clone()).unwrap();
    admin.receive(other).unwrap();
    assert!(admin.state().store.is_forked(&bob.me(), one.seq).unwrap());
    admin.sync().await;
    let proof = store
        .keys_under(&format!("ops/forks/{}/", bob.my_id()))
        .await
        .pop()
        .expect("the proof went up");
    (one.seq, proof)
}

/// One unreadable object among the fork proofs doesn't stop the others
/// being learned, and it's given up after a few tries rather than
/// downloaded on every pass.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_fork_proof_doesnt_stop_the_others() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, carol) = on_a_flaky_store(root.path()).await;
    let (seq, _) = a_shared_fork(&admin, &bob, &store).await;
    // At another seq, listed first, so learning the real fork doesn't
    // settle it.
    store
        .plant_unreadable(
            &format!("ops/forks/{}/{:020}/{}", bob.my_id(), 0, "0".repeat(64)),
            b"junk",
        )
        .await;

    carol.sync().await;
    assert!(carol.state().store.is_forked(&bob.me(), seq).unwrap());
    for _ in 0..4 {
        carol.sync().await;
    }

    let downloads = store.refused_reads.load(std::sync::atomic::Ordering::SeqCst);
    assert!(downloads <= 3, "downloaded {downloads} times");
}

/// Our own fork proof made unreadable in the bucket is put back, without
/// first needing to read what's there.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_copy_of_our_fork_proof_is_put_back() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket, admin, bob, _carol) = on_a_flaky_store(root.path()).await;
    let (_, proof) = a_shared_fork(&admin, &bob, &store).await;
    store.plant_unreadable(&proof, b"junk").await;

    admin.sync().await;

    assert!(bucket.fork_proof(&proof).await.is_ok(), "readable again");
}

/// A head that can't be read is settled once its author's log is read, not
/// downloaded again at every check until it changes.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_head_is_read_once_its_log_is() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, _carol) = on_a_flaky_store(root.path()).await;
    store
        .plant_unreadable(&format!("heads/{}", bob.my_id()), b"junk")
        .await;

    for _ in 0..4 {
        admin.check().await;
    }

    let downloads = store.refused_reads.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(downloads, 1);
}

/// One log that can't be listed doesn't stop the sweep reading the others.
/// Logs are swept in no set order, so each member in turn is the one that
/// fails, and one of the two rounds always fails before the other's log.
#[tokio::test(flavor = "multi_thread")]
async fn one_log_that_cant_be_listed_doesnt_stop_the_sweep() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, carol) = on_a_flaky_store(root.path()).await;
    let general = channel_id(&admin, "general");
    for (failing, other) in [(&bob, &carol), (&carol, &bob)] {
        let text = format!("from {} while {} fails", other.my_id(), failing.my_id());
        other.send_message(&general, &text, None).unwrap();
        other.sync().await;
        *store.fail_next_list_under.lock().unwrap() = Some(format!("ops/{}", failing.my_id()));

        admin.sync().await;

        assert!(texts_in(&admin, &general).contains(&text), "{text}");
    }
}

/// A member's head that can't be read doesn't stop the check reading the
/// others' logs, and that member's log is read anyway.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_head_doesnt_stop_reading_the_others() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, carol) = on_a_flaky_store(root.path()).await;
    // Heads are checked in key order; the first one is the unreadable one.
    let (first, second) = match bob.my_id() < carol.my_id() {
        true => (&bob, &carol),
        false => (&carol, &bob),
    };
    let general = channel_id(&admin, "general");
    first.send_message(&general, "from the first", None).unwrap();
    first.sync().await;
    second.send_message(&general, "from the second", None).unwrap();
    second.sync().await;
    store
        .plant_unreadable(&format!("heads/{}", first.my_id()), b"junk")
        .await;

    admin.check().await;

    let texts = texts_in(&admin, &general);
    assert!(texts.contains(&"from the second".to_owned()));
    assert!(
        texts.contains(&"from the first".to_owned()),
        "its log is read anyway"
    );
}

/// Our own head that can't be read is written again after an upload,
/// rather than stopping the upload's head for good.
#[tokio::test(flavor = "multi_thread")]
async fn our_own_unreadable_head_is_written_again() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket, admin, bob, _carol) = on_a_flaky_store(root.path()).await;
    store
        .plant_unreadable(&format!("heads/{}", bob.my_id()), b"junk")
        .await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "landed", None).unwrap();

    bob.sync().await;

    let head = bucket.head(&bob.me()).await.expect("readable again");
    let pushed = bob.state().store.last_pushed_seq(&bob.me()).unwrap();
    assert_eq!(head.map(|head| head.last_seq), Some(pushed));
}

/// Right after a removal, the removed member makes the commit slot
/// unreadable, so members who were offline can't follow and would keep
/// writing under the key the removed member has. Commit repair writes the
/// commit over it, without first needing to read what's there.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_commit_slot_is_repaired() {
    let root = tempfile::tempdir().unwrap();
    let (store, _bucket, admin, bob, carol) = on_a_flaky_store(root.path()).await;
    admin.remove_member(&carol.my_id()).await.unwrap();
    let slot = store.keys_under("mls/commit/").await.pop().unwrap();
    store.plant_unreadable(&slot, b"junk").await;

    admin.sync().await;
    bob.sync().await;

    assert_eq!(
        bob.state().mls.epoch().unwrap(),
        admin.state().mls.epoch().unwrap(),
        "Bob followed the removal"
    );
}

/// A gap whose repair failed on a network blip is tried again at the next
/// check, not only at the hourly sweep.
#[tokio::test(flavor = "multi_thread")]
async fn a_gap_repair_cut_short_by_the_network_is_tried_again() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
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
    admin.check().await;
    // As if the parked ops were pushed out by the cap.
    admin.state().store.execute_for_test("DELETE FROM pending_ops");
    // Bob's head counts as seen once a check reads past it, so only the gap
    // repair can bring the lost ops back.
    admin.check().await;
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

    *store.fail_next_list_under.lock().unwrap() = Some(format!("ops/{}", bob.my_id()));
    admin.check().await;
    assert!(
        store.fail_next_list_under.lock().unwrap().is_none(),
        "the repair hit the blip"
    );
    assert!(admin.status().error.is_some(), "and failed");
    admin.check().await;

    let texts: Vec<_> = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(texts.contains(&"second".to_string()), "{texts:?}");
}

/// A removed member's frozen log can end with ops they only ever sent live,
/// which the bucket never gets. Readers missing them stop listing that
/// member at every check once nothing new has come; the hourly sweep still
/// looks.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_that_cannot_be_finished_is_not_read_at_every_check() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "only live", None).unwrap();
    for envelope in bob.state().store.unpushed(&bob.me()).unwrap() {
        admin.receive(envelope).unwrap();
    }
    admin.remove_member(&bob.my_id()).await.unwrap();
    carol.check().await;
    carol.check().await;

    let before = store.requests();
    carol.check().await;
    let settled_check = store.requests().since(&before);

    assert_eq!(settled_check.list_pages(), 1, "only heads: {settled_check}");
}

/// A commit slot's name is predictable, so junk could be put there before
/// the admin's next commit. Commits are plain puts, so the commit replaces
/// it and still reaches every member.
#[tokio::test(flavor = "multi_thread")]
async fn junk_put_first_at_a_commit_slot_does_not_block_the_commit() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let next_slot = admin.state().mls.epoch().unwrap();
    bucket
        .put_raw(&format!("mls/commit/{next_slot:020}"), b"junk".to_vec())
        .await
        .unwrap();

    admin.remove_member(&carol.my_id()).await.unwrap();
    bob.sync().await;

    let carol_is_active = bob
        .members()
        .unwrap()
        .into_iter()
        .any(|member| member.id == carol.my_id() && member.active);
    assert!(!carol_is_active, "the removal reached Bob");
}

/// Junk that waited for its epoch key (kept with the parked ops) is skipped
/// the same way once the key is there, so its author's later ops apply.
/// So is a body that decodes but has no clock, which only junk can have.
#[tokio::test(flavor = "multi_thread")]
async fn junk_kept_waiting_or_without_a_clock_is_skipped_too() {
    for is_kept_waiting in [true, false] {
        let (_root, _bucket, admin, bob) = admin_and_bob().await;
        let general = channel_id(&admin, "general");
        let (epoch, key) = current_key(&bob);
        let next = bob.state().store.next_seq(&bob.me()).unwrap();
        let without_a_clock = OpBody {
            lamport: 0,
            timestamp_ms: 0,
            op: Op::Message {
                id: crate::ops::new_owned_id(&bob.me()),
                channel: general.clone(),
                text: "no clock".into(),
                reply_to: None,
            },
        };
        let junk = match is_kept_waiting {
            true => crypto::seal_bytes(
                &bob.inner.identity,
                &key,
                epoch,
                next,
                postcard::to_stdvec(&(crypto::OP_FORMAT, "not an op")).unwrap(),
            )
            .unwrap(),
            false => crypto::seal(&bob.inner.identity, &key, epoch, next, &without_a_clock).unwrap(),
        };
        let after = OpBody {
            lamport: 9_001,
            timestamp_ms: 0,
            op: Op::Message {
                id: crate::ops::new_owned_id(&bob.me()),
                channel: general.clone(),
                text: "after the junk".into(),
                reply_to: None,
            },
        };
        if is_kept_waiting {
            admin.state().store.insert_pending(&junk).unwrap();
        } else {
            admin.receive(junk).unwrap();
        }
        admin
            .receive(crypto::seal(&bob.inner.identity, &key, epoch, next + 1, &after).unwrap())
            .unwrap();
        admin.sync().await;
        admin.sync().await;

        let texts: Vec<_> = admin
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .map(|message| message.text)
            .collect();
        assert!(
            texts.contains(&"after the junk".to_string()),
            "kept waiting: {is_kept_waiting}, {texts:?}"
        );
    }
}

/// Packs only speed up reading, so one the bucket refuses (say, a proxy's
/// upload size limit) is reported once and not tried again: later packs
/// still go up, and nothing is uploaded twice.
#[tokio::test(flavor = "multi_thread")]
async fn a_pack_the_bucket_refuses_is_reported_once_and_holds_up_nothing() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let general = channel_id(&admin, "general");
    let pack = |first: u64, last: u64| {
        format!(
            "ops/{}/{last:020}-{first:020}-",
            crate::views::member_id(&bob.me())
        )
    };
    bob.push().await.unwrap();
    *store.refuse_puts_under.lock().unwrap() = Some(pack(1, 16));

    let mut failed_pushes = 0;
    while bob.state().store.last_seq(&bob.me()).unwrap() < 40 {
        bob.send_message(&general, "filler", None).unwrap();
        failed_pushes += usize::from(bob.push().await.is_err());
    }

    let listed = bucket.list_segments(&bob.me(), None).await.unwrap();
    let uploads_of = |prefix: String| {
        listed
            .iter()
            .filter(|object| object.key.starts_with(&prefix))
            .count()
    };
    assert_eq!(failed_pushes, 1, "the refused pack is reported once");
    assert_eq!((uploads_of(pack(1, 16)), uploads_of(pack(17, 32))), (0, 1));
}

/// Anyone with the bucket keys can name an object as if it held a wide range
/// of someone's log, so a reader tries it before the honest packs. After a
/// few such failures the reader gives up on the chain and reads the rest in
/// listing order, where those names sort last: they cost a download each,
/// can't hold up the honest history, and hide nothing.
#[tokio::test(flavor = "multi_thread")]
async fn names_planted_as_wide_packs_hide_nothing_and_hold_up_little() {
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
    let start = bob.state().store.envelopes_between(&bob.me(), 1, 8).unwrap();
    let genuine_but_short = postcard::to_stdvec(&start).unwrap();
    let mut planted = HashMap::new();
    for number in 0..10u8 {
        let nonce = [number; 16];
        let (name, bytes) = match number % 2 {
            0 => (
                segment_name_for(&bob.me(), 1, 1_000_000, &nonce, b"other"),
                b"junk".to_vec(),
            ),
            _ => (
                segment_name_for(&bob.me(), 1, 1_000_000, &nonce, &genuine_but_short),
                genuine_but_short.clone(),
            ),
        };
        bucket.put_raw(&name, bytes).await.unwrap();
        planted.insert(name, 0);
    }
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let fetched_before = store.fetched_keys().len();

    carol.sync().await;

    assert_eq!(
        carol.state().store.last_seq(&bob.me()).unwrap(),
        bob.state().store.last_seq(&bob.me()).unwrap(),
        "the newcomer read Bob's whole log"
    );
    let bob_log = format!("ops/{}/", crate::views::member_id(&bob.me()));
    let fetched: Vec<String> = store.fetched_keys()[fetched_before..]
        .iter()
        .filter(|key| key.starts_with(&bob_log))
        .cloned()
        .collect();
    let planted_first = fetched
        .iter()
        .take_while(|key| planted.contains_key(*key))
        .count();
    assert!(
        planted_first <= 3,
        "{planted_first} planted names came before Bob's history"
    );
    for key in &fetched {
        if let Some(count) = planted.get_mut(key) {
            *count += 1;
        }
    }
    assert!(planted.values().all(|count| *count <= 1), "{planted:?}");
}

/// Anyone with the bucket keys can plant an envelope "from" a member for an
/// epoch whose key hasn't come yet, so it is parked unchecked. It mustn't
/// count as an op waiting behind a missing one: gap repair would then fetch
/// that member's junk objects again every time they write (#215).
#[tokio::test(flavor = "multi_thread")]
async fn a_forged_parked_op_doesnt_make_gap_repair_fetch_junk_again() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    sync_all(&[&admin, &bob]).await;
    let general = channel_id(&admin, "general");
    let held = admin.state().store.last_seq(&bob.me()).unwrap();
    let junk = segment_name_for(&bob.me(), held + 1_000, held + 1_000, &[3; 16], b"other");
    bucket.put_raw(&junk, b"junk".to_vec()).await.unwrap();
    let epoch = admin.state().mls.epoch().unwrap();
    admin
        .receive(Envelope {
            author: bob.me(),
            seq: held + 500,
            epoch: epoch + 1,
            nonce: [0; 24],
            ciphertext: vec![1; 64],
        })
        .unwrap();

    for number in 0..3 {
        bob.send_message(&general, &format!("message {number}"), None)
            .unwrap();
        bob.push().await.unwrap();
        admin.sync().await;
    }

    assert!(texts_in(&admin, &general).contains(&"message 2".to_string()));
    let junk_downloads = store.fetched_keys().iter().filter(|key| **key == junk).count();
    assert_eq!(junk_downloads, 1, "fetched once per run");
}

/// Three names planted as wide packs shouldn't make a newcomer fall back to
/// downloading every honest pack as well as every segment: that is several
/// times the bytes the log takes, more than before packs existed.
#[tokio::test(flavor = "multi_thread")]
async fn three_planted_names_dont_make_a_newcomer_download_every_pack_too() {
    let (with_planted, ops_in_log) = honest_ops_downloaded_by_a_newcomer(3).await;
    let (without, _) = honest_ops_downloaded_by_a_newcomer(0).await;
    assert!(
        with_planted <= ops_in_log + ops_in_log / 2,
        "{with_planted} ops downloaded for a log of {ops_in_log} (without planted names: {without})"
    );
}

/// How many of Bob's ops (summed over the ranges of his honest objects) a
/// newcomer downloads with `planted` junk names claiming his whole log, and
/// how long his log is.
async fn honest_ops_downloaded_by_a_newcomer(planted: u8) -> (u64, u64) {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let general = channel_id(&admin, "general");
    for number in 0..128 {
        bob.send_message(&general, &format!("message {number}"), None)
            .unwrap();
        bob.push().await.unwrap();
    }
    admin.sync().await;
    let mut planted_names = Vec::new();
    for number in 0..planted {
        let name = segment_name_for(&bob.me(), 1, 1_000_000, &[number; 16], b"other");
        bucket.put_raw(&name, b"junk".to_vec()).await.unwrap();
        planted_names.push(name);
    }
    let fetched_before = store.fetched_keys().len();
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    carol.sync().await;
    let ops_in_log = bob.state().store.last_seq(&bob.me()).unwrap();
    assert_eq!(carol.state().store.last_seq(&bob.me()).unwrap(), ops_in_log);
    let downloaded = store.fetched_keys()[fetched_before..]
        .iter()
        .filter(|key| !planted_names.contains(key))
        .filter_map(|key| parse_segment_name(&bob.me(), key))
        .map(|name| name.last - name.first + 1)
        .sum();
    (downloaded, ops_in_log)
}

/// Once the chain is given up (here, after three names planted as wide
/// packs), objects that narrower ones cover are read last, but names alone
/// never leave one out: junk named as narrower segments over an honest one
/// can't stand in for it, and the honest one is still read in the same pass.
#[tokio::test(flavor = "multi_thread")]
async fn junk_named_as_narrower_segments_cant_stand_in_for_an_honest_one() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let held = bob.state().store.last_seq(&bob.me()).unwrap();
    for number in 0..3 {
        bob.send_message(&general, &format!("together {number}"), None)
            .unwrap();
    }
    bob.push().await.unwrap();
    for seq in held + 1..=held + 3 {
        let nonce = [u8::try_from(seq).unwrap(); 16];
        let name = segment_name_for(&bob.me(), seq, seq, &nonce, b"junk");
        bucket.put_raw(&name, b"junk".to_vec()).await.unwrap();
    }
    for number in 0..3u8 {
        let name = segment_name_for(&bob.me(), 1, 1_000_000, &[200 + number; 16], b"wide");
        bucket.put_raw(&name, b"junk".to_vec()).await.unwrap();
    }

    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    carol.sync().await;

    assert_eq!(
        carol.state().store.last_seq(&bob.me()).unwrap(),
        bob.state().store.last_seq(&bob.me()).unwrap()
    );
}

/// An op that waited for its epoch key sits in an object skipped as
/// unsettled. Once a commit brings a new key, skipped objects are read again
/// (junk too, once per new key), so such an op isn't left until a restart
/// even if forged parked ops pushed every real one out.
#[tokio::test(flavor = "multi_thread")]
async fn skipped_objects_are_read_again_once_a_new_epoch_key_arrives() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = super::capacity::counting_store::counting_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    sync_all(&[&admin, &bob]).await;
    let held = bob.state().store.last_seq(&admin.me()).unwrap();
    let junk = segment_name_for(&admin.me(), held + 1_000, held + 1_000, &[4; 16], b"other");
    bucket.put_raw(&junk, b"junk".to_vec()).await.unwrap();
    let junk_downloads_by_bob = async || {
        let before = store.fetched_keys().len();
        bob.sync().await;
        store.fetched_keys()[before..]
            .iter()
            .filter(|key| **key == junk)
            .count()
    };
    assert_eq!(junk_downloads_by_bob().await, 1);
    assert_eq!(junk_downloads_by_bob().await, 0, "skipped while nothing changes");

    admin.remove_member(&dave.my_id()).await.unwrap();

    assert_eq!(junk_downloads_by_bob().await, 1);
}

/// `author` sends `text` live to `holders`, and their device dies before it
/// reaches the bucket. Returns the op.
async fn send_live_then_die(
    store: &FlakyStore,
    author: &Workspace,
    holders: &[&Workspace],
    text: &str,
) -> Envelope {
    *store.refuse_puts_under.lock().unwrap() =
        Some(format!("ops/{}/", crate::views::member_id(&author.me())));
    let general = channel_id(author, "general");
    author.send_message(&general, text, None).unwrap();
    let envelope = author
        .state()
        .store
        .unpushed(&author.me())
        .unwrap()
        .last()
        .unwrap()
        .clone();
    for holder in holders {
        holder.receive(envelope.clone()).unwrap();
    }
    assert!(author.shutdown().await.is_err(), "the upload never happened");
    *store.refuse_puts_under.lock().unwrap() = None;
    envelope
}

fn see_each_other_online(members: &[&Workspace]) {
    for from in members {
        let beat = from.heartbeat(PresenceState::Online).unwrap();
        for to in members.iter().filter(|to| to.me() != from.me()) {
            to.receive_heartbeat(&beat).unwrap();
        }
    }
}

/// Checks enough times, with enough time passing, for every member in line
/// to have had their turn to relay.
async fn let_relays_happen(members: &[&Workspace]) {
    use crate::sync::relay::{RELAY_AFTER_MS, RELAY_STEP_MS};
    for member in members {
        member.check().await;
    }
    let waits = std::iter::once(RELAY_AFTER_MS).chain(std::iter::repeat_n(RELAY_STEP_MS, members.len()));
    for wait in waits {
        // All at once, as members' checks would be.
        let checks: Vec<_> = members
            .iter()
            .map(|member| {
                member.backdate_relay_waits(wait);
                let member = (*member).clone();
                tokio::spawn(async move { member.check().await })
            })
            .collect();
        for check in checks {
            check.await.unwrap();
        }
    }
}

fn objects_of(listed: &[crate::transport::bucket::ListedObject], author: &Workspace) -> usize {
    let prefix = format!("ops/{}/", crate::views::member_id(&author.me()));
    listed
        .iter()
        .filter(|object| object.key.starts_with(&prefix))
        .count()
}

/// The admin's device dies right after sending an op live. A member who got
/// it relays it to the bucket, so the admin restored from the kit continues
/// after it instead of reusing its seq (which every device would prove a
/// fork), and a newcomer gets it too.
#[tokio::test(flavor = "multi_thread")]
async fn ops_only_sent_live_are_relayed_so_a_restore_continues_after_them() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let kit = admin.recovery_kit().await.unwrap();
    let admin_id = admin.me();
    let general = channel_id(&admin, "general");
    let live_only = send_live_then_die(&store, &admin, &[&bob, &carol], "only sent live").await;
    drop(admin);

    see_each_other_online(&[&bob, &carol]);
    let_relays_happen(&[&bob, &carol]).await;
    let restored = Workspace::restore_with(
        &data_dir(&root.path().join("ada-new-laptop")),
        &kit,
        connector(&bucket),
        Networking::Offline,
    )
    .await
    .unwrap();

    assert_eq!(restored.state().store.last_seq(&admin_id).unwrap(), live_only.seq);
    restored
        .send_message(&general, "after the restore", None)
        .unwrap();
    sync_all(&[&restored, &bob]).await;
    let texts = texts_in(&bob, &general);
    assert!(
        texts.contains(&"only sent live".to_string()) && texts.contains(&"after the restore".to_string())
    );
    for seq in [live_only.seq, live_only.seq + 1] {
        assert!(!bob.state().store.is_forked(&admin_id, seq).unwrap(), "seq {seq}");
    }
    let dave = join(&restored, &root.path().join("dave"), &bucket, "Dave", false).await;
    dave.sync().await;
    assert!(texts_in(&dave, &general).contains(&"only sent live".to_string()));
}

/// Members online wait their turn by how close their key is to the
/// author's and look in the bucket first, so five members holding the same
/// live-only op don't all upload it.
#[tokio::test(flavor = "multi_thread")]
async fn five_members_holding_the_same_live_only_op_relay_it_about_once() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let mut others = Vec::new();
    for name in ["Carol", "Dave", "Erin", "Frank"] {
        others.push(join(&admin, &root.path().join(name), &bucket, name, false).await);
    }
    let holders: Vec<&Workspace> = std::iter::once(&admin).chain(others.iter()).collect();
    let everyone: Vec<&Workspace> = std::iter::once(&bob).chain(holders.iter().copied()).collect();
    sync_all(&everyone).await;
    let before = objects_of(&bucket.list_segments(&bob.me(), None).await.unwrap(), &bob);

    send_live_then_die(&store, &bob, &holders, "only sent live").await;
    see_each_other_online(&holders);
    let_relays_happen(&holders).await;

    let relayed = objects_of(&bucket.list_segments(&bob.me(), None).await.unwrap(), &bob) - before;
    assert!((1..=2).contains(&relayed), "{relayed} relay uploads");
}

/// Junk put under the author's name, even names claiming exactly the
/// missing op, can't make a member think it is already in the bucket.
#[tokio::test(flavor = "multi_thread")]
async fn junk_under_an_author_neither_stops_nor_spoils_a_relay() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");

    let live_only = send_live_then_die(&store, &bob, &[&carol], "only sent live").await;
    let seq = live_only.seq;
    let mismatched = segment_name_for(&bob.me(), seq, seq, &[1; 16], b"other");
    bucket.put_raw(&mismatched, b"junk".to_vec()).await.unwrap();
    let matching_junk = segment_name_for(&bob.me(), seq, seq, &[2; 16], b"junk");
    bucket.put_raw(&matching_junk, b"junk".to_vec()).await.unwrap();
    let_relays_happen(&[&carol]).await;

    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    dave.sync().await;
    assert!(texts_in(&dave, &general).contains(&"only sent live".to_string()));
}

/// Only ops a member verified and applied are relayed, never parked ones,
/// and each only once: a deleted head can't make members upload again.
#[tokio::test(flavor = "multi_thread")]
async fn only_applied_ops_are_relayed_and_only_once() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let bob_objects = async || objects_of(&bucket.list_segments(&bob.me(), None).await.unwrap(), &bob);
    let before = bob_objects().await;

    *store.refuse_puts_under.lock().unwrap() = Some(format!("ops/{}/", crate::views::member_id(&bob.me())));
    bob.send_message(&general, "first", None).unwrap();
    bob.send_message(&general, "second", None).unwrap();
    let unpushed = bob.state().store.unpushed(&bob.me()).unwrap();
    let (first, second) = (
        unpushed[unpushed.len() - 2].clone(),
        unpushed[unpushed.len() - 1].clone(),
    );
    assert!(bob.shutdown().await.is_err());
    *store.refuse_puts_under.lock().unwrap() = None;
    carol.receive(second).unwrap();
    let_relays_happen(&[&carol]).await;
    assert_eq!(bob_objects().await, before, "a parked op isn't relayed");

    carol.receive(first).unwrap();
    let_relays_happen(&[&carol]).await;
    assert_eq!(bob_objects().await, before + 1);

    let bob_log = format!("ops/{}", crate::views::member_id(&bob.me()));
    bucket.delete_head(&bob.me()).await.unwrap();
    let listed_before = store.listings_of(&bob_log);
    let_relays_happen(&[&carol]).await;
    assert_eq!(
        store.listings_of(&bob_log),
        listed_before,
        "not even looked for again"
    );
    assert_eq!(bob_objects().await, before + 1, "relayed once");
}

/// An op refused for its size is kept only as its header, which is no op at
/// all for anyone else, so it isn't relayed; the ops around it are.
#[tokio::test(flavor = "multi_thread")]
async fn ops_kept_without_their_content_are_not_relayed() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let message = |text: String| Op::Message {
        id: crate::ops::new_owned_id(&bob.me()),
        channel: general.clone(),
        text,
        reply_to: None,
    };
    let mut forger = Forger::new(&bob);
    let huge = forger.seal(message("x".repeat(crate::ops::MAX_OP_BYTES)));
    let after = forger.seal(message("after the huge one".into()));
    carol.receive(huge.clone()).unwrap();
    carol.receive(after.clone()).unwrap();

    let_relays_happen(&[&carol]).await;

    let relayed: Vec<_> = bucket
        .list_segments(&bob.me(), None)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|object| parse_segment_name(&bob.me(), &object.key))
        .filter(|name| name.last >= huge.seq)
        .map(|name| (name.first, name.last))
        .collect();
    assert_eq!(relayed, [(after.seq, after.seq)]);
}

/// Ops a member read from the bucket are there already: missing their hint
/// (asleep, or gossip down) must not make it look for them again.
#[tokio::test(flavor = "multi_thread")]
async fn ops_read_from_the_bucket_are_not_looked_for_again() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");

    bob.send_message(&general, "uploaded as usual", None).unwrap();
    bob.check().await;
    carol.check().await;
    assert!(texts_in(&carol, &general).contains(&"uploaded as usual".to_string()));

    let bob_log = format!("ops/{}", crate::views::member_id(&bob.me()));
    let listed_before = store.listings_of(&bob_log);
    let_relays_happen(&[&carol]).await;
    assert_eq!(store.listings_of(&bob_log), listed_before);
}

/// Reading a later op from the bucket shows the bucket has that op, not the
/// ones before it, which may have come only live: those are still relayed.
#[tokio::test(flavor = "multi_thread")]
async fn a_segment_read_past_a_live_only_op_doesnt_stop_its_relay() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let live_only = send_live_then_die(&store, &bob, &[&carol], "only sent live").await;
    let mut forger = Forger::new(&bob);
    let later = forger.seal(Op::Message {
        id: crate::ops::new_owned_id(&bob.me()),
        channel: general.clone(),
        text: "in the bucket".into(),
        reply_to: None,
    });
    assert_eq!(later.seq, live_only.seq + 1);
    bucket.put_segment(std::slice::from_ref(&later)).await.unwrap();
    carol.sync().await;
    assert_eq!(carol.state().store.last_seq(&bob.me()).unwrap(), later.seq);

    let_relays_happen(&[&carol]).await;

    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    dave.sync().await;
    assert!(texts_in(&dave, &general).contains(&"only sent live".to_string()));
}

/// A relay that fails (the bucket is down) waits its turn again, so after
/// a long outage the members in line don't all upload at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_relay_waits_its_turn_again() {
    use crate::sync::relay::RELAY_AFTER_MS;
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let bob_objects = async || objects_of(&bucket.list_segments(&bob.me(), None).await.unwrap(), &bob);
    send_live_then_die(&store, &bob, &[&carol], "only sent live").await;
    let before = bob_objects().await;
    let bob_log = format!("ops/{}/", crate::views::member_id(&bob.me()));

    *store.refuse_puts_under.lock().unwrap() = Some(bob_log.clone());
    let_relays_happen(&[&carol]).await;
    *store.refuse_puts_under.lock().unwrap() = None;
    carol.check().await;
    assert_eq!(bob_objects().await, before, "not straight after the outage");

    carol.backdate_relay_waits(RELAY_AFTER_MS);
    carol.check().await;
    assert_eq!(bob_objects().await, before + 1);
}

/// One author's relay failing doesn't hold up relaying the others'.
#[tokio::test(flavor = "multi_thread")]
async fn one_failing_relay_doesnt_stop_the_others() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let erin = join(&admin, &root.path().join("erin"), &bucket, "Erin", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &erin, &carol]).await;
    send_live_then_die(&store, &bob, &[&carol], "bob's").await;
    send_live_then_die(&store, &erin, &[&carol], "erin's").await;
    // Whichever of the two is relayed first fails, every time.
    let authors = carol.state().store.everyone_ever().unwrap();
    let position = |member: &Workspace| authors.iter().position(|author| *author == member.me()).unwrap();
    let (failing, other) = match position(&bob) < position(&erin) {
        true => (&bob, &erin),
        false => (&erin, &bob),
    };
    let before = objects_of(&bucket.list_segments(&other.me(), None).await.unwrap(), other);
    *store.refuse_puts_under.lock().unwrap() =
        Some(format!("ops/{}/", crate::views::member_id(&failing.me())));

    let_relays_happen(&[&carol]).await;

    let after = objects_of(&bucket.list_segments(&other.me(), None).await.unwrap(), other);
    assert_eq!(after, before + 1);
}

/// Junk with real headers under the author, read in a sweep, is no proof
/// the bucket holds the op: anyone with the bucket keys could put it there.
#[tokio::test(flavor = "multi_thread")]
async fn junk_with_real_headers_read_in_a_sweep_doesnt_stop_a_relay() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let live_only = send_live_then_die(&store, &bob, &[&carol], "only sent live").await;

    let mut junk = live_only.clone();
    junk.ciphertext = vec![7; live_only.ciphertext.len()];
    let bytes = postcard::to_stdvec(&vec![junk]).unwrap();
    let name = segment_name_for(&bob.me(), live_only.seq, live_only.seq, &[9; 16], &bytes);
    bucket.put_raw(&name, bytes).await.unwrap();
    carol.sync().await;
    let_relays_happen(&[&carol]).await;

    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    dave.sync().await;
    assert!(texts_in(&dave, &general).contains(&"only sent live".to_string()));
}

/// A removed member keeps the old bucket key but never learns the new one:
/// it travels in an op under the epoch key the removal replaced. Once the
/// old key is deleted at the provider, everyone else carries on with the
/// new key, and the app proves the old one dead.
#[tokio::test(flavor = "multi_thread")]
async fn a_removed_member_never_learns_the_new_bucket_key() {
    use super::key_rotation::{FIRST_KEY, Provider, admin_bob_carol, change_key};
    let provider = Provider::new();
    let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
    let key_of = |member: &Workspace| {
        crate::config::current_bucket(&member.state().store)
            .unwrap()
            .access_key_id
    };
    admin.remove_member(&carol.my_id()).await.unwrap();
    change_key(&admin, "new-key").await;
    sync_all(&[&admin, &bob, &carol]).await;
    assert_eq!(
        (key_of(&bob), key_of(&carol)),
        ("new-key".to_string(), FIRST_KEY.to_string())
    );

    provider.delete_key(FIRST_KEY);
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "after the old key went", None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(texts_in(&admin, &general).contains(&"after the old key went".to_string()));
    assert_eq!(
        admin.check_old_key_deleted().await.unwrap(),
        crate::views::OldKeyCheck::Dead
    );
}

fn reactions_on(member: &crate::Workspace, channel: &str, message: &str) -> Vec<String> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .find(|shown| shown.id == message)
        .expect("the message is shown")
        .reactions
        .into_iter()
        .map(|reaction| reaction.emoji)
        .collect()
}

#[tokio::test]
async fn a_member_cant_flood_a_message_with_reactions() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "news", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let faces: Vec<String> = ('\u{1F600}'..='\u{1F63F}').map(String::from).collect();
    let mut forger = Forger::new(&bob);
    for face in &faces {
        admin
            .receive(forger.seal(Op::Reaction {
                message: message.clone(),
                emoji: face.clone(),
                active: true,
            }))
            .unwrap();
    }
    let shown = reactions_on(&admin, &general, &message);
    assert_eq!(shown, faces[..crate::ops::MAX_REACTIONS_PER_AUTHOR]);

    // Taking one back makes room for another.
    for (emoji, active) in [(&faces[0], false), (&faces[40], true)] {
        admin
            .receive(forger.seal(Op::Reaction {
                message: message.clone(),
                emoji: emoji.clone(),
                active,
            }))
            .unwrap();
    }
    let shown = reactions_on(&admin, &general, &message);
    assert_eq!(shown.len(), crate::ops::MAX_REACTIONS_PER_AUTHOR);
    assert!(shown.contains(&faces[40]) && !shown.contains(&faces[0]));
    // Everyone else can still react.
    admin.react(&message, "👍", true).unwrap();
    assert!(reactions_on(&admin, &general, &message).contains(&"👍".to_owned()));
}

#[tokio::test]
async fn reactions_and_status_emoji_cant_read_as_words() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = admin.send_message(&general, "news", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    // Regional-indicator letters spell words when joined or chained, not just flags.
    for label in [
        "✓ approved by admin",
        "(you)",
        "Admin",
        "ok",
        "1",
        "#",
        "🇴\u{200D}🇰",
        "🇴🇰🇦🇾",
        "🇫",
    ] {
        assert!(bob.react(&message, label, true).is_err(), "{label}");
        let status = crate::views::StatusUpdate {
            emoji: label,
            text: "",
            until_ms: None,
            alerts: crate::views::NotifyLevel::All,
        };
        assert!(bob.set_status(status).is_err(), "{label}");
    }
    for emoji in ["👍🏽", "1️⃣", "🇫🇷", "👨‍👩‍👧", "❤️"] {
        bob.react(&message, emoji, true).unwrap();
    }
    sync_all(&[&admin, &bob]).await;
    assert_eq!(reactions_on(&admin, &general, &message).len(), 5);

    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal(Op::Reaction {
            message: message.clone(),
            emoji: "✓ approved by admin".into(),
            active: true,
        }))
        .unwrap();
    admin
        .receive(forger.seal(Op::Status {
            emoji: "(you)".into(),
            text: String::new(),
            until_ms: None,
            alerts: crate::ops::StatusAlerts::All,
        }))
        .unwrap();
    assert_eq!(
        reactions_on(&admin, &general, &message).len(),
        5,
        "the label wasn't added"
    );
    let bob_seen = admin
        .members()
        .unwrap()
        .into_iter()
        .find(|member| member.id == bob.my_id())
        .unwrap();
    assert!(bob_seen.status.is_none_or(|status| status.emoji != "(you)"));
}

/// A sync pass can upload a commit it finds missing. If a removal could
/// record its commit while a pass runs, the pass could upload it before the
/// removal uploads the recovery snapshot that covers it; a device lost in
/// between leaves a kit that restore refuses for good. So membership
/// changes wait for a pass in progress.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_waits_for_a_sync_pass_in_progress() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let admin = Arc::new(admin);
    let pass_running = admin.inner.sync_lock.lock().await;
    let removing = tokio::spawn({
        let admin = admin.clone();
        let bob_id = bob.my_id();
        async move { admin.remove_member(&bob_id).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        admin.current_members().unwrap().contains(&bob.me()),
        "the removal went ahead during the pass"
    );

    drop(pass_running);
    removing.await.unwrap().unwrap();

    assert!(!admin.current_members().unwrap().contains(&bob.me()));
}

/// An approval waits for a sync pass in progress, for the same reason.
#[tokio::test(flavor = "multi_thread")]
async fn an_approval_waits_for_a_sync_pass_in_progress() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = Arc::new(create_admin(&root.path().join("ada"), &bucket).await);
    let invite = admin
        .create_test_invite("Bob", std::time::Duration::from_secs(600))
        .unwrap();
    let token = invite.token.clone();
    let joining = start_join(invite, &root.path().join("bob"), &bucket, "Bob", false);
    wait_for_request(&admin, &token).await;
    let members_before = admin.current_members().unwrap().len();
    let pass_running = admin.inner.sync_lock.lock().await;
    let approving = tokio::spawn({
        let admin = admin.clone();
        async move { admin.approve_invite(&token).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(
        admin.current_members().unwrap().len(),
        members_before,
        "the approval went ahead during the pass"
    );

    drop(pass_running);
    approving.await.unwrap().unwrap();

    assert_eq!(admin.current_members().unwrap().len(), members_before + 1);
    joining.await.unwrap().unwrap();
}

/// Keys found by trying seeds until two safety codes began alike, as someone
/// about to join could, to pass for a member who shares their name.
fn seeded_key(seed: u64) -> iroh::SecretKey {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    iroh::SecretKey::from_bytes(&bytes)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_joiner_whose_safety_code_starts_like_a_members_is_refused() {
    let (bob_key, mallory_key) = (seeded_key(2479), seeded_key(9890));
    let shown =
        |key: &iroh::SecretKey| crate::views::shown_safety_code(&crate::views::safety_code(&key.public()));
    assert_eq!(
        shown(&bob_key),
        shown(&mallory_key),
        "the two keys' codes start alike"
    );

    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob_dir = root.path().join("bob");
    data_dir(&bob_dir).use_identity_for_test(&bob_key).await.unwrap();
    let bob = join(&admin, &bob_dir, &bucket, "Grace", false).await;
    assert_eq!(bob.me(), bob_key.public());

    let mallory_dir = root.path().join("mallory");
    data_dir(&mallory_dir)
        .use_identity_for_test(&mallory_key)
        .await
        .unwrap();
    let invite = admin
        .create_test_invite("", std::time::Duration::from_secs(600))
        .unwrap();
    let token = invite.token.clone();
    let _joining = start_join(invite, &mallory_dir, &bucket, "Grace", false);
    wait_for_request(&admin, &token).await;
    let refused = admin.approve_invite(&token).await.unwrap_err();
    assert!(format!("{refused:#}").contains("safety code"), "{refused:#}");
    assert!(
        !admin
            .members()
            .unwrap()
            .iter()
            .any(|member| member.id == hex::encode(mallory_key.public().as_bytes()))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forged_decline_is_ignored_and_the_join_goes_on() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob_dir = root.path().join("bob");
    let code = data_dir(&bob_dir).safety_code().await.unwrap();
    let invite = admin
        .create_test_invite("", std::time::Duration::from_secs(600))
        .unwrap();
    let token = invite.token.clone();
    let joining = start_join(invite, &bob_dir, &bucket, "Bob", false);
    wait_for_request(&admin, &token).await;

    // Anyone with the bucket keys can write the answer slot.
    let forged = crate::membership::invite::Answer::Rejected {
        signature: vec![7; 64],
    };
    bucket
        .put_invite_answer(&token, postcard::to_stdvec(&forged).unwrap())
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert!(!joining.is_finished(), "a forged decline doesn't end the join");

    admin.approve_invite(&token).await.unwrap();
    let bob = joining.await.unwrap().unwrap();
    assert_eq!(
        bob.safety_code(),
        code,
        "the key the admin approved is still there"
    );
}

/// Only the admin sets the workspace's icon and look: anyone else's op is
/// ignored everywhere, and the app refuses to send one.
#[tokio::test(flavor = "multi_thread")]
async fn only_the_admin_sets_the_workspace_icon_and_look() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    assert!(bob.set_workspace_look(Some("dusk"), None).is_err());
    let icon = [b"\x89PNG\r\n\x1a\n".as_slice(), b"bob"].concat();
    assert!(bob.set_workspace_icon(Some(icon)).await.is_err());

    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal(Op::WorkspaceLook {
            theme: Some("dusk".into()),
            accent: None,
        }))
        .unwrap();
    admin
        .receive(forger.seal(Op::WorkspaceIcon { picture: None }))
        .unwrap();

    assert_eq!(
        admin.workspace_look().unwrap(),
        crate::views::WorkspaceLook::default()
    );
    assert_eq!(admin.workspace_icon_id().unwrap(), None);
}

/// Look names are plain lowercase words of at most 24 letters: anything
/// else is refused before sending and skipped when it arrives.
#[tokio::test(flavor = "multi_thread")]
async fn workspace_look_names_are_bounded() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    for bad in ["", "Dusk", "dusk-2", "dusk mode", "<b>", &"a".repeat(25)] {
        assert!(admin.set_workspace_look(Some(bad), None).is_err(), "{bad:?}");
        assert!(admin.set_workspace_look(None, Some(bad)).is_err(), "{bad:?}");
    }
    let longest = "a".repeat(24);
    admin.set_workspace_look(Some(&longest), Some("violet")).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(bob.workspace_look().unwrap().theme, Some(longest.clone()));

    let forged = Forger::new(&admin).seal(Op::WorkspaceLook {
        theme: Some("x".repeat(10_000)),
        accent: None,
    });
    bob.receive(forged).unwrap();

    assert_eq!(bob.workspace_look().unwrap().theme, Some(longest));
}

/// The admin's latest look wins in any order and when applied twice, and a
/// tie (only a forged op makes one) ends the same everywhere.
#[tokio::test(flavor = "multi_thread")]
async fn the_workspace_look_merges_the_same_in_any_order() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let mut forger = Forger::new(&admin);
    let look = |theme: &str| Op::WorkspaceLook {
        theme: Some(theme.into()),
        accent: None,
    };
    let older = forger.seal_at(100, look("dawn"));
    let newer = forger.seal_at(200, look("dusk"));
    let tied_one = forger.seal_at(300, look("moss"));
    let tied_other = forger.seal_at(300, look("sand"));

    for envelope in [&older, &newer, &tied_one, &tied_other] {
        bob.receive(envelope.clone()).unwrap();
    }
    for envelope in [&tied_other, &tied_one, &newer, &older, &newer] {
        carol.receive(envelope.clone()).unwrap();
    }
    // Ops that arrived ahead of their turn wait until a pass retries them.
    bob.sync().await;
    carol.sync().await;

    assert_eq!(bob.workspace_look().unwrap(), carol.workspace_look().unwrap());
    assert_eq!(bob.workspace_look().unwrap().theme.as_deref(), Some("sand"));
}

/// An app that predates the workspace look can't read its op: it keeps it
/// as unreadable and goes on with the admin's later ops. Once updated, its
/// views are rebuilt under the new rules, and the look applies.
#[tokio::test(flavor = "multi_thread")]
async fn a_look_kept_as_unreadable_applies_after_updating() {
    use crate::storage::VIEW_RULES_META;
    let (root, bucket, admin, bob) = admin_and_bob().await;
    admin.set_workspace_look(Some("dusk"), None).unwrap();
    let look = admin
        .state()
        .store
        .unpushed(&admin.me())
        .unwrap()
        .last()
        .unwrap()
        .clone();
    // What v0.1.0 does with an op it can't read: kept, never applied; and
    // the view rules it recorded.
    bob.state().store.insert_applied(&look, true).unwrap();
    bob.state().store.set_meta(VIEW_RULES_META, &[1]).unwrap();
    assert_eq!(
        bob.workspace_look().unwrap(),
        crate::views::WorkspaceLook::default()
    );
    bob.shutdown().await.unwrap();
    drop(bob);

    let updated = Workspace::open_with(
        &data_dir(&root.path().join("bob")),
        connector(&bucket),
        Networking::Offline,
    )
    .await
    .unwrap();

    assert_eq!(updated.workspace_look().unwrap().theme.as_deref(), Some("dusk"));
}

/// A member removed while on a page goes from it at once, and nothing they
/// send to it afterwards shows.
#[tokio::test(flavor = "multi_thread")]
async fn a_removed_member_goes_from_a_page_and_cant_come_back_to_it() {
    use super::page_cursors::{beacon, cursor};
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.create_page("Plans", None).unwrap();
    admin.join_page(&page, true).await.unwrap();
    admin.receive_page(&beacon(&bob, &page, true)).unwrap();
    admin.receive_page(&cursor(&bob, &page, 1)).unwrap();

    admin.remove_member(&bob.my_id()).await.unwrap();

    let presence = admin.page_presence(&page).unwrap();
    assert!(presence.viewers.is_empty() && presence.cursors.is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert!(admin.receive_page(&beacon(&bob, &page, true)).is_err());
    assert!(admin.receive_page(&cursor(&bob, &page, 2)).is_err());
    assert!(admin.page_presence(&page).unwrap().viewers.is_empty());
}

/// A beacon or cursor signed for another workspace doesn't show here, even
/// from a member: signatures name the workspace, so one can't be replayed.
#[tokio::test(flavor = "multi_thread")]
async fn page_messages_signed_for_another_workspace_are_refused() {
    use crate::{ops::now_ms, pages::cursors::PageBeacon, transport::page_gossip::PageWire};
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.create_page("Plans", None).unwrap();
    admin.join_page(&page, true).await.unwrap();
    let elsewhere = PageWire::Beacon(
        PageBeacon::signed(
            &bob.inner.identity,
            "another workspace",
            &page,
            true,
            now_ms().unwrap(),
        )
        .unwrap(),
    );

    assert!(admin.receive_page(&elsewhere).is_err());
    assert!(admin.page_presence(&page).unwrap().viewers.is_empty());
}

/// A mention naming another workspace isn't one here, even of an id that
/// is a member here: it can't notify, alert or count.
#[tokio::test(flavor = "multi_thread")]
async fn a_mention_naming_another_workspace_doesnt_mention_anyone_here() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob]).await;
    if let Some(latest) = admin.messages(&general, None, 1).unwrap().pop() {
        admin.mark_read(&general, &latest.id).unwrap();
    }
    let elsewhere = "fedcba9876543210";
    assert_ne!(admin.workspace_id(), elsewhere);
    let member = crate::links::app_link(elsewhere, "member", &admin.my_id());
    let everyone = crate::links::app_link(elsewhere, "mention", "channel");
    bob.send_message(
        &general,
        &format!("[@Ada]({member}) and [@channel]({everyone})"),
        None,
    )
    .unwrap();
    sync_all(&[&admin, &bob]).await;

    let channel = admin
        .channels()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == general)
        .unwrap();
    assert_eq!(channel.unread_mentions, 0);
    assert!(
        !admin
            .messages(&general, None, 1)
            .unwrap()
            .pop()
            .unwrap()
            .mentions_me
    );
}

/// The desktop editor's form of a mention of `whom`.
fn page_mention_of(whom: &Workspace) -> String {
    format!(
        "[@them]({})",
        crate::links::app_link(&whom.inner.config.workspace_id, "member", &whom.my_id())
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_flood_of_page_mentions_alerts_a_few_times_and_keeps_one_inbox_entry() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = bob.create_page("Plan", None).unwrap();
    bob.edit_page(&page, crate::pages::Edit::Append(&"filler line\n".repeat(2_000)))
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let mut changes = admin.subscribe();

    for round in 0..100 {
        let text = format!("{round} {}", page_mention_of(&admin));
        bob.edit_page(&page, crate::pages::Edit::Append(&text)).unwrap();
    }
    // Text that only looks like a mention costs nothing beyond reading the update.
    let lookalike = format!("member/{} shy://member/{}0", admin.my_id(), admin.my_id());
    for _ in 0..200 {
        bob.edit_page(&page, crate::pages::Edit::Append(&lookalike))
            .unwrap();
    }
    sync_all(&[&admin, &bob]).await;

    let mut alerts = 0;
    while let Ok(change) = changes.try_recv() {
        alerts += usize::from(matches!(change, crate::views::Change::PageAlert { .. }));
    }
    assert_eq!(alerts, 3);
    let inbox = admin.inbox().unwrap();
    assert_eq!(inbox.page_mentions.len(), 1);
    assert!(
        inbox.page_mentions[0].preview.starts_with("99 "),
        "{:?}",
        inbox.page_mentions[0].preview
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_member_gets_no_page_mentions_from_after_their_removal() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let page = bob.create_page("Plan", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    admin.remove_member(&carol.my_id()).await.unwrap();
    sync_all(&[&admin, &bob]).await;
    bob.edit_page(&page, crate::pages::Edit::Append(&page_mention_of(&carol)))
        .unwrap();
    bob.sync().await;
    let mut changes = carol.subscribe();
    let _ = carol.sync().await;

    while let Ok(change) = changes.try_recv() {
        assert!(
            !matches!(change, crate::views::Change::PageAlert { .. }),
            "{change:?}"
        );
    }
    assert!(carol.inbox().unwrap().page_mentions.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_insert_packed_with_mentions_is_read_in_linear_time() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = bob.create_page("Plan", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    // No `[` anywhere: a backward search for each mention's label would
    // scan the whole insert again for every one of them.
    let packed = format!("](shy://member/{}", admin.my_id()).repeat(20_000);
    bob.edit_page(&page, crate::pages::Edit::Append(&packed)).unwrap();

    let started = std::time::Instant::now();
    sync_all(&[&admin, &bob]).await;

    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    assert_eq!(admin.inbox().unwrap().page_mentions.len(), 1);
}

mod storage_key_handoff {
    //! Storage settings one member hands another whose bucket key stopped
    //! working (see `sync/key_handoff.rs`).

    use iroh::SecretKey;

    use super::{
        super::key_rotation::{FIRST_KEY, Provider, admin_bob_carol, change_key},
        *,
    };
    use crate::sync::key_handoff::{KeyAnswer, KeyRequest};

    fn key_in_use(member: &Workspace) -> String {
        member.storage_settings().unwrap().access_key_id
    }

    /// What Bob hands Carol for `request`, which must be settings.
    fn settings_from(
        bob: &Workspace,
        carol: &Workspace,
        request: &KeyRequest,
    ) -> (Vec<(u64, Vec<u8>)>, Envelope) {
        match bob.key_answer_for(&carol.me(), request).unwrap() {
            KeyAnswer::Settings { commits, settings } => (commits, settings),
            other => panic!("expected settings, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn storage_settings_not_signed_by_the_admin_are_refused() {
        let provider = Provider::new();
        let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
        let mut bucket = carol.bucket_config().unwrap();
        bucket.access_key_id = "bobs-key".into();
        bucket.secret_access_key = "bobs-secret".into();
        let signed_by_bob = Forger::new(&bob).seal_at(1 << 20, Op::StorageSettings { bucket });
        let mut claiming_the_admin = signed_by_bob.clone();
        claiming_the_admin.author = admin.me();

        for settings in [signed_by_bob, claiming_the_admin] {
            let handed = KeyAnswer::Settings {
                commits: Vec::new(),
                settings,
            };
            assert!(carol.take_key_answer(handed).is_err());
            assert_eq!(key_in_use(&carol), FIRST_KEY);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn forged_or_missing_commits_handed_over_change_nothing() {
        let provider = Provider::new();
        let (root, admin, bob, carol) = admin_bob_carol(&provider).await;
        let dave = join_connected(
            &admin,
            &root.path().join("dave"),
            &provider.bucket(),
            provider.connector(),
            "Dave",
        )
        .await;
        sync_all(&[&admin, &bob, &carol, &dave]).await;
        let epoch = dave.state().mls.epoch().unwrap();
        let request = dave.key_request().unwrap();

        admin.remove_member(&carol.my_id()).await.unwrap();
        admin.remove_member(&bob.my_id()).await.unwrap();
        change_key(&admin, "second").await;
        let (commits, settings) = settings_from(&admin, &dave, &request);
        assert_eq!(commits.len(), 2);

        let forged = vec![(epoch, b"not a commit".to_vec()), commits[1].clone()];
        let only_the_second = vec![commits[1].clone()];
        for commits in [forged, only_the_second] {
            let handed = KeyAnswer::Settings {
                commits,
                settings: settings.clone(),
            };
            assert!(dave.take_key_answer(handed).is_err());
            assert_eq!(dave.state().mls.epoch().unwrap(), epoch);
            assert_eq!(key_in_use(&dave), FIRST_KEY);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn older_storage_settings_handed_over_change_nothing() {
        let provider = Provider::new();
        let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
        let first = carol.key_request().unwrap();
        change_key(&admin, "second").await;
        sync_all(&[&admin, &bob]).await;
        let older = settings_from(&bob, &carol, &first);
        change_key(&admin, "third").await;
        sync_all(&[&admin, &bob, &carol]).await;
        assert_eq!(key_in_use(&carol), "third");

        let handed = KeyAnswer::Settings {
            commits: older.0,
            settings: older.1,
        };
        assert!(!carol.take_key_answer(handed).unwrap());
        assert_eq!(key_in_use(&carol), "third");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn only_another_current_member_is_answered() {
        let provider = Provider::new();
        let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
        let request = KeyRequest {
            epoch: 0,
            settings_lamport: 0,
        };
        change_key(&admin, "second").await;
        sync_all(&[&admin, &bob]).await;

        let stranger = SecretKey::generate().public();
        assert!(bob.key_answer_for(&stranger, &request).is_err());
        assert!(bob.key_answer_for(&bob.me(), &request).is_err());
        assert!(bob.key_answer_for(&carol.me(), &request).is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_member_asking_again_and_again_is_answered_once_a_minute() {
        let provider = Provider::new();
        let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;

        assert!(bob.may_answer_key_request(&carol.me()));
        assert!(!bob.may_answer_key_request(&carol.me()));
        assert!(
            bob.may_answer_key_request(&admin.me()),
            "others are still answered"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handed_over_settings_carry_no_readable_credentials() {
        let provider = Provider::new();
        let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
        let request = carol.key_request().unwrap();
        change_key(&admin, "second").await;
        sync_all(&[&admin, &bob]).await;
        let (commits, settings) = settings_from(&bob, &carol, &request);
        let answer = KeyAnswer::Settings { commits, settings };

        let on_the_wire = postcard::to_stdvec(&answer).unwrap();
        let secret = b"second-secret";
        assert!(!on_the_wire.windows(secret.len()).any(|window| window == secret));
        let printed = format!("{answer:?}");
        assert!(!printed.contains("second"), "{printed}");
        assert!(printed.len() < 80, "only counts are printed: {printed}");
    }
}

mod sealed_join_requests {
    //! Join requests wait in the bucket sealed to the admin (see
    //! `membership/request_seal.rs`).

    use std::time::Duration;

    use super::*;
    use crate::{
        membership::{
            invite::Invite,
            request_seal::{RequestSlot, open_request},
        },
        views::{self, InviteState},
    };

    fn invite_view(admin: &Workspace, token: &str) -> views::Invite {
        admin
            .invites()
            .unwrap()
            .into_iter()
            .find(|invite| invite.token == token)
            .unwrap()
    }

    fn new_invite(admin: &Workspace) -> Invite {
        admin.create_test_invite("", Duration::from_secs(600)).unwrap()
    }

    /// Starts Bob joining with `invite` and returns his request as it sits
    /// in the bucket.
    async fn posted_request(invite: Invite, root: &std::path::Path, bucket: &Bucket) -> Vec<u8> {
        let token = invite.token.clone();
        let _joining = start_join(invite, &root.join("bob"), bucket, "Bob Secretname", false);
        for _ in 0..200 {
            if let Some(bytes) = bucket.invite_request(&token).await.unwrap() {
                return bytes.to_vec();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the request never reached the bucket");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_request_in_the_bucket_doesnt_show_the_joiners_name() {
        let root = tempfile::tempdir().unwrap();
        let bucket = memory_bucket();
        let admin = create_admin(&root.path().join("ada"), &bucket).await;
        let invite = new_invite(&admin);
        let token = invite.token.clone();

        let in_the_bucket = posted_request(invite, root.path(), &bucket).await;

        let name = b"Secretname";
        assert!(!in_the_bucket.windows(name.len()).any(|window| window == name));
        wait_for_request(&admin, &token).await;
        assert_eq!(
            invite_view(&admin, &token).requester_name.as_deref(),
            Some("Bob Secretname")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_request_moved_to_another_invite_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let bucket = memory_bucket();
        let admin = create_admin(&root.path().join("ada"), &bucket).await;
        let first = new_invite(&admin);
        let second = new_invite(&admin).token;
        let first_token = first.token.clone();
        let sealed = posted_request(first, root.path(), &bucket).await;

        bucket.put_invite_request(&second, sealed).await.unwrap();
        admin.sync().await;

        assert_eq!(invite_view(&admin, &second).state, InviteState::Open);
        assert_eq!(invite_view(&admin, &first_token).state, InviteState::Requested);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_request_in_the_clear_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let bucket = memory_bucket();
        let admin = create_admin(&root.path().join("ada"), &bucket).await;
        let first = new_invite(&admin);
        let second = new_invite(&admin).token;
        let slot = RequestSlot {
            workspace_id: &first.workspace_id,
            token: &first.token.clone(),
            admin: &first.admin.clone(),
        };
        let sealed = posted_request(first.clone(), root.path(), &bucket).await;
        let request = open_request(&admin.inner.identity, slot, &sealed).unwrap();

        let in_the_clear = postcard::to_stdvec(&request).unwrap();
        bucket.put_invite_request(&second, in_the_clear).await.unwrap();
        admin.sync().await;

        assert_eq!(invite_view(&admin, &second).state, InviteState::Open);
    }

    /// Deleting what doesn't open could delete a genuine request posted
    /// meanwhile; the joiner's upload replaces it instead.
    #[tokio::test(flavor = "multi_thread")]
    async fn junk_in_a_request_slot_is_left_for_the_joiner_to_replace() {
        let root = tempfile::tempdir().unwrap();
        let bucket = memory_bucket();
        let admin = create_admin(&root.path().join("ada"), &bucket).await;
        let invite = new_invite(&admin);
        let token = invite.token.clone();
        bucket.put_invite_request(&token, b"junk".to_vec()).await.unwrap();

        admin.sync().await;

        assert_eq!(
            bucket.invite_request(&token).await.unwrap().as_deref(),
            Some(b"junk".as_slice())
        );
        assert_eq!(invite_view(&admin, &token).state, InviteState::Open);
        assert_eq!(admin.status().error, None);
        posted_request(invite, root.path(), &bucket).await;
        wait_for_request(&admin, &token).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_oversized_join_request_isnt_read() {
        let root = tempfile::tempdir().unwrap();
        let bucket = memory_bucket();
        let admin = create_admin(&root.path().join("ada"), &bucket).await;
        let token = new_invite(&admin).token;
        bucket
            .put_invite_request(&token, vec![0; 1024 * 1024])
            .await
            .unwrap();

        admin.sync().await;

        assert_eq!(invite_view(&admin, &token).state, InviteState::Open);
        assert_eq!(admin.status().error, None);
    }
}

mod huddle_audience {
    //! Who hears a huddle's audio follows removals at once.

    use std::time::Duration;

    use crate::{
        tests::{
            huddles::{carry, three_members},
            support::sync_all,
        },
        workspace::Via,
    };

    /// An audience worked out before a removal mustn't be published after the
    /// one worked out since, or the removed member is heard until the next tick.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_removal_during_an_audience_refresh_takes_effect_at_once() {
        let (_root, [ada, bob, carol]) = three_members().await;
        let plans = ada
            .workspace
            .create_private_channel("plans", &[bob.workspace.my_id()])
            .unwrap();
        sync_all(&[&ada.workspace, &bob.workspace, &carol.workspace]).await;
        ada.workspace.join_huddle(&plans, false).unwrap();
        bob.workspace.join_huddle(&plans, false).unwrap();
        carry(&bob.workspace, &[&ada.workspace], Via::Direct(bob.workspace.me()));
        tokio::time::timeout(Duration::from_secs(10), async {
            while ada.media.peers().len() != 1 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("Ada never heard Bob");

        let (paused, is_paused) = std::sync::mpsc::channel();
        let (resume, resumed) = std::sync::mpsc::channel();
        *ada.workspace.inner.audience_pause.lock().unwrap() = Some((paused, resumed));
        let stale = std::thread::spawn({
            let ada = ada.workspace.clone();
            move || ada.follow_membership()
        });
        is_paused.recv().unwrap();
        ada.workspace
            .remove_from_private_channel(&plans, &bob.workspace.my_id())
            .unwrap();
        // Time for the session to refresh on the removal, if nothing holds it back.
        tokio::time::sleep(Duration::from_millis(300)).await;
        resume.send(()).unwrap();
        stale.join().unwrap().unwrap();

        tokio::time::timeout(Duration::from_secs(2), async {
            while !ada.media.peers().is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the removed member was still offered audio");
    }
}

/// A store that says an object is small doesn't get to send more than the
/// limit: the cap holds while the body is read.
#[tokio::test(flavor = "multi_thread")]
async fn an_object_larger_than_its_stated_size_is_cut_off_while_reading() {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket, admin, _bob, _carol) = on_a_flaky_store(root.path()).await;
    let token = admin
        .create_test_invite("", std::time::Duration::from_secs(600))
        .unwrap()
        .token;
    bucket
        .put_invite_request(&token, vec![0; 1024 * 1024])
        .await
        .unwrap();
    store
        .understates_sizes
        .store(true, std::sync::atomic::Ordering::SeqCst);

    assert_eq!(bucket.invite_request(&token).await.unwrap(), None);
    bucket
        .put_invite_request(&token, b"small".to_vec())
        .await
        .unwrap();
    assert_eq!(
        bucket.invite_request(&token).await.unwrap().as_deref(),
        Some(b"small".as_slice()),
        "what fits still reads"
    );
}

/// Mentions of you are kept one per page and author, and an update may
/// name any page; one member mentioning you in page after page keeps only
/// their newest, and can't push anyone else's out.
#[tokio::test(flavor = "multi_thread")]
async fn a_member_mentioning_you_in_many_pages_cant_fill_the_inbox() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let carols_page = carol.create_page("Notes", None).unwrap();
    carol
        .edit_page(&carols_page, crate::pages::Edit::Append(&page_mention_of(&admin)))
        .unwrap();
    sync_all(&[&admin, &carol]).await;

    for round in 0..60 {
        let page = bob.create_page(&format!("Spam {round}"), None).unwrap();
        bob.edit_page(&page, crate::pages::Edit::Append(&page_mention_of(&admin)))
            .unwrap();
    }
    sync_all(&[&admin, &bob]).await;

    let mentions = admin.inbox().unwrap().page_mentions;
    let from = |member: &Workspace| {
        mentions
            .iter()
            .filter(|mention| mention.author == member.my_id())
            .count()
    };
    assert_eq!(from(&bob), 50, "only Bob's newest");
    assert_eq!(from(&carol), 1, "Carol's stays");
}
