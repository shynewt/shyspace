//! Attacks on private channels and DMs, from members, former members and
//! outsiders. Every rule in the "Private channels" part of docs/security.md
//! has a test here.

use super::support::*;
use crate::{
    Workspace,
    ops::Op,
    private::{
        crypto::{Granting, commitment, new_channel_key, wrap_key},
        ops::{GrantBody, PrivateOp},
    },
    storage::StoredKey,
    transport::bucket::Bucket,
};

fn texts(member: &Workspace, channel: &str) -> Vec<String> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect()
}

fn newest_key(member: &Workspace, channel: &str) -> StoredKey {
    member
        .state()
        .store
        .channel_keys(Some(channel))
        .unwrap()
        .pop()
        .unwrap()
}

fn message(member: &Workspace, channel: &str, text: &str) -> Op {
    Op::Message {
        id: crate::private::ids::new_private_message_id(channel, &member.me()),
        channel: channel.to_owned(),
        text: text.into(),
        reply_to: None,
    }
}

/// A grant from `granter` to `recipient` of `key` as `channel`'s `generation`.
fn forged_grant(
    granter: &Workspace,
    recipient: &Workspace,
    channel: &str,
    generation: u64,
    key: [u8; 32],
) -> Op {
    let granting = Granting {
        workspace_id: &granter.inner.config.workspace_id,
        granter: &granter.me(),
    };
    let public = granter
        .state()
        .store
        .sealing_key_of(&recipient.me())
        .unwrap()
        .unwrap();
    let grant = GrantBody {
        channel: channel.to_owned(),
        generation,
        key,
        dm_members: Vec::new(),
    };
    Op::KeyGrant {
        wraps: vec![wrap_key(granting, &public, &postcard::to_stdvec(&grant).unwrap()).unwrap()],
    }
}

async fn four_members() -> (
    tempfile::TempDir,
    Bucket,
    Workspace,
    Workspace,
    Workspace,
    Workspace,
) {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    (root, bucket, admin, bob, carol, dave)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sealed_payload_copied_into_another_op_opens_nowhere() {
    let (_root, _bucket, admin, bob, carol, _dave) = four_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    let original = bob.send_message(&channel, "original", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let seq = bob.state().store.last_seq(&bob.me()).unwrap();
    let Op::Sealed { payload } = bob.state().store.stored_op(&bob.me(), seq).unwrap().body.op else {
        panic!("Bob's last op is sealed");
    };

    // The admin can't read it, but replays it as their own op.
    carol
        .receive(Forger::new(&admin).seal(Op::Sealed { payload }))
        .unwrap();
    let messages = carol.messages(&channel, None, 50).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, original);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_the_owner_never_announced_is_never_trusted_or_used() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    // Carol, a member but not the owner, makes up generation 2 and tries to
    // vouch for it herself.
    let fake = new_channel_key();
    let fake_key = StoredKey {
        channel: channel.clone(),
        generation: 2,
        key: fake,
    };
    let current = newest_key(&carol, &channel);
    let mut forger = Forger::new(&carol);
    dave.receive(forger.seal(forged_grant(&carol, &dave, &channel, 2, fake)))
        .unwrap();
    dave.receive(forger.seal_private(
        &current,
        PrivateOp::Rotate {
            generation: 2,
            commitment: commitment(&fake),
            owner_index: 0,
        },
    ))
    .unwrap();
    dave.receive(forger.seal_private(
        &fake_key,
        PrivateOp::Content(message(&carol, &channel, "WHISPER")),
    ))
    .unwrap();

    assert!(
        texts(&dave, &channel).is_empty(),
        "nothing under the fake key counts"
    );
    assert_eq!(dave.state().store.latest_generation(&channel).unwrap(), 1);
    dave.send_message(&channel, "sealed with the real key", None)
        .unwrap();
    sync_all(&[&bob, &dave]).await;
    assert_eq!(texts(&bob, &channel), ["sealed with the real key"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_creator_its_id_names_can_start_a_channel() {
    let (_root, _bucket, admin, bob, carol, _dave) = four_members().await;
    let key = new_channel_key();
    let channel = crate::private::ids::new_private_channel_id(&bob.me());
    let first = StoredKey {
        channel: channel.clone(),
        generation: 1,
        key,
    };
    let mut forger = Forger::new(&carol);
    admin
        .receive(forger.seal(forged_grant(&carol, &admin, &channel, 1, key)))
        .unwrap();
    admin
        .receive(forger.seal_private(
            &first,
            PrivateOp::Create {
                name: "bobs-secrets".into(),
                commitment: commitment(&key),
                members: Vec::new(),
            },
        ))
        .unwrap();
    assert!(
        admin
            .channels()
            .unwrap()
            .iter()
            .all(|listed| listed.id != channel)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn key_generations_count_up_one_at_a_time() {
    let (_root, _bucket, admin, bob, carol, _dave) = four_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    let jump = new_channel_key();
    let current = newest_key(&bob, &channel);
    let mut forger = Forger::new(&bob);
    carol
        .receive(forger.seal(forged_grant(&bob, &carol, &channel, 9, jump)))
        .unwrap();
    carol
        .receive(forger.seal_private(
            &current,
            PrivateOp::Rotate {
                generation: 9,
                commitment: commitment(&jump),
                owner_index: 0,
            },
        ))
        .unwrap();
    assert_eq!(carol.state().store.latest_generation(&channel).unwrap(), 1);
    assert_eq!(
        newest_key(&carol, &channel).generation,
        9,
        "held, but not vouched for"
    );
    carol
        .send_message(&channel, "still on generation 1", None)
        .unwrap();
    let sent = carol.state().store.last_seq(&carol.me()).unwrap();
    assert!(carol.state().store.is_private_applied(&carol.me(), sent).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_racing_its_authors_removal_ends_up_nowhere() {
    let (_root, _bucket, admin, bob, carol, _dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), admin.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    carol.send_message(&channel, "RACING", None).unwrap();
    carol.sync().await;
    admin.sync().await;
    assert_eq!(texts(&admin, &channel), ["RACING"]);
    // Bob removes Carol before he has seen her message.
    bob.remove_from_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    assert!(texts(&bob, &channel).is_empty());
    assert!(texts(&admin, &channel).is_empty(), "undone where it had applied");
    assert!(admin.search("RACING", 10).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn ops_opened_late_end_up_the_same_as_in_order() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let bob_before = bob.state().store.last_seq(&bob.me()).unwrap();
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let carol_before = carol.state().store.last_seq(&carol.me()).unwrap();
    carol.send_message(&channel, "one", None).unwrap();
    carol.send_message(&channel, "two", None).unwrap();
    bob.add_to_private_channel(&channel, &dave.my_id()).unwrap();

    // Dave gets Carol's messages before Bob's grant and add.
    for envelope in carol
        .state()
        .store
        .envelopes_after(&carol.me(), carol_before)
        .unwrap()
    {
        dave.receive(envelope).unwrap();
    }
    assert!(texts(&dave, &channel).is_empty());
    for envelope in bob.state().store.envelopes_after(&bob.me(), bob_before).unwrap() {
        dave.receive(envelope).unwrap();
    }
    assert_eq!(texts(&dave, &channel), ["one", "two"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn what_someone_wrote_while_removed_never_counts_after_they_rejoin() {
    let (_root, _bucket, admin, bob, carol, _dave) = four_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let old_key = newest_key(&carol, &channel);
    bob.remove_from_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    let written_while_removed = Forger::new(&carol).seal_private(
        &old_key,
        PrivateOp::Content(message(&carol, &channel, "WHILE-REMOVED")),
    );
    carol.receive(written_while_removed.clone()).unwrap();
    bob.receive(written_while_removed).unwrap();
    bob.add_to_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert!(!texts(&bob, &channel).contains(&"WHILE-REMOVED".to_string()));
    carol.send_message(&channel, "back properly", None).unwrap();
    sync_all(&[&bob, &carol]).await;
    assert_eq!(texts(&bob, &channel), ["back properly"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn outsiders_cannot_join_a_dm_by_granting_themselves_into_it() {
    let (_root, _bucket, admin, bob, carol, _dave) = four_members().await;
    let dm = bob.open_dm(&[carol.my_id()]).unwrap();
    bob.send_message(&dm, "just us", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    // The admin claims a key for Bob and Carol's DM, listing themselves in.
    let key = new_channel_key();
    let granting = Granting {
        workspace_id: &admin.inner.config.workspace_id,
        granter: &admin.me(),
    };
    let public = admin.state().store.sealing_key_of(&carol.me()).unwrap().unwrap();
    let grant = GrantBody {
        channel: dm.clone(),
        generation: 0,
        key,
        dm_members: vec![admin.me(), bob.me(), carol.me()],
    };
    let wrap = wrap_key(granting, &public, &postcard::to_stdvec(&grant).unwrap()).unwrap();
    let mut forger = Forger::new(&admin);
    carol
        .receive(forger.seal(Op::KeyGrant { wraps: vec![wrap] }))
        .unwrap();
    let forged_key = StoredKey {
        channel: dm.clone(),
        generation: 0,
        key,
    };
    carol
        .receive(forger.seal_private(&forged_key, PrivateOp::Content(message(&admin, &dm, "INTRUDER"))))
        .unwrap();
    assert_eq!(texts(&carol, &dm), ["just us"]);
    assert_eq!(carol.state().store.dm_members(&dm).unwrap().len(), 2);
}
