//! Attacks on private channels and DMs, from members, former members and
//! outsiders. Every rule in the "Private channels" part of docs/security.md
//! has a test here.

use super::support::*;
use crate::{
    Workspace,
    ops::{Envelope, Op},
    private::{
        crypto::{Granting, commitment, new_channel_key, wrap_key},
        ids::{dm_id, private_channel_id},
        ops::{GrantBody, Invitee, PrivateOp},
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

/// The creation of `key`'s channel, sealed by its creator as their next op.
fn forged_create(forger: &mut Forger, key: &StoredKey, name: &str, members: &[&Workspace]) -> Envelope {
    let members = members
        .iter()
        .map(|member| Invitee {
            member: member.me(),
            after_seq: own_last_seq(member),
        })
        .collect();
    forger.seal_private(
        key,
        PrivateOp::Create {
            name: name.into(),
            commitment: commitment(&key.key),
            members,
        },
    )
}

fn own_last_seq(member: &Workspace) -> u64 {
    member.state().store.last_seq(&member.me()).unwrap()
}

/// Hands `from`'s ops after `after` straight to `to`, as gossip would, so a
/// test decides the order each device hears things in.
fn deliver(from: &Workspace, after: u64, to: &Workspace) {
    for envelope in from.state().store.envelopes_after(&from.me(), after).unwrap() {
        to.receive(envelope).unwrap();
    }
}

fn owner(member: &Workspace, channel: &str) -> Option<String> {
    member.private_members(channel).unwrap().owner
}

fn members_of(member: &Workspace, channel: &str) -> Vec<String> {
    member.private_members(channel).unwrap().members
}

fn listed(member: &Workspace, channel: &str) -> Option<crate::views::Channel> {
    member
        .channels()
        .unwrap()
        .into_iter()
        .find(|listed| listed.id == channel)
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
            position: 2,
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
    let channel = private_channel_id(&bob.me(), &commitment(&key), "bobs-secrets", &[]).unwrap();
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
                position: 2,
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

#[tokio::test(flavor = "multi_thread")]
async fn a_key_granted_under_another_label_hides_nothing() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    bob.rotate_private_channel_key(&channel).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    // Carol hands Dave the channel's real key again, labelled as an older
    // generation of it and as another channel, hoping his device files Bob's
    // ops under the wrong label and drops them.
    let real = newest_key(&carol, &channel);
    let mut forger = Forger::new(&carol);
    for (label, generation) in [(channel.as_str(), 1), ("p:!", 1)] {
        dave.receive(forger.seal(forged_grant(&carol, &dave, label, generation, real.key)))
            .unwrap();
    }
    let bob_before = own_last_seq(&bob);
    bob.send_message(&channel, "hello dave", None).unwrap();
    deliver(&bob, bob_before, &dave);
    assert_eq!(texts(&dave, &channel), ["hello dave"]);
    assert!(listed(&dave, "p:!").is_none());

    let dave_before = own_last_seq(&dave);
    dave.send_message(&channel, "hello bob", None).unwrap();
    deliver(&dave, dave_before, &bob);
    assert_eq!(texts(&bob, &channel), ["hello dave", "hello bob"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hand_over_racing_the_new_owners_leave_converges() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id(), admin.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    let (bob_before, carol_before) = (own_last_seq(&bob), own_last_seq(&carol));
    carol.leave_private_channel(&channel).unwrap();
    bob.hand_over_private_channel(&channel, &carol.my_id()).unwrap();
    // Dave hears the hand-over first, the admin hears Carol leave first.
    deliver(&bob, bob_before, &dave);
    deliver(&carol, carol_before, &dave);
    deliver(&carol, carol_before, &admin);
    deliver(&bob, bob_before, &admin);

    assert_eq!(owner(&dave, &channel), owner(&admin, &channel));
    assert_eq!(members_of(&dave, &channel), members_of(&admin, &channel));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_owners_delete_counts_where_the_hand_over_arrives_later() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id(), admin.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    let daves = dave.send_message(&channel, "from dave", None).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    let bob_before = own_last_seq(&bob);
    bob.hand_over_private_channel(&channel, &carol.my_id()).unwrap();
    deliver(&bob, bob_before, &carol);
    let carol_before = own_last_seq(&carol);
    carol.delete_message(&daves).unwrap();
    // The admin hears Carol's delete before Bob's hand-over; Dave in order.
    deliver(&carol, carol_before, &admin);
    deliver(&bob, bob_before, &admin);
    deliver(&bob, bob_before, &dave);
    deliver(&carol, carol_before, &dave);

    assert_eq!(texts(&admin, &channel), texts(&dave, &channel));
    assert!(texts(&dave, &channel).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_former_owners_delete_survives_a_rebuild() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let everyone = [&admin, &bob, &carol, &dave];
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id(), admin.my_id()])
        .unwrap();
    sync_all(&everyone).await;
    let carols = carol
        .send_message(&channel, "DELETED-BY-OLD-OWNER", None)
        .unwrap();
    sync_all(&everyone).await;
    bob.delete_message(&carols).unwrap();
    bob.hand_over_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&everyone).await;
    assert!(texts(&dave, &channel).is_empty());

    // Dave applies a message of the admin's that Carol, the new owner, hasn't
    // seen when she removes the admin, so Dave rebuilds his views.
    let admin_before = own_last_seq(&admin);
    admin.send_message(&channel, "RACING", None).unwrap();
    deliver(&admin, admin_before, &dave);
    assert_eq!(texts(&dave, &channel), ["RACING"]);
    let carol_before = own_last_seq(&carol);
    carol
        .remove_from_private_channel(&channel, &admin.my_id())
        .unwrap();
    deliver(&carol, carol_before, &dave);

    assert!(texts(&dave, &channel).is_empty(), "{:?}", texts(&dave, &channel));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removal_by_an_owner_behind_on_the_members_log_still_removes_them() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let everyone = [&admin, &bob, &carol, &dave];
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&everyone).await;

    // Bob has seen Dave's latest public messages when he adds him; Carol,
    // who then takes the channel over, hasn't.
    let general = channel_id(&admin, "general");
    let dave_before = own_last_seq(&dave);
    for index in 0..3 {
        dave.send_message(&general, &format!("public {index}"), None)
            .unwrap();
    }
    deliver(&dave, dave_before, &bob);
    let bob_before = own_last_seq(&bob);
    bob.add_to_private_channel(&channel, &dave.my_id()).unwrap();
    bob.hand_over_private_channel(&channel, &carol.my_id()).unwrap();
    deliver(&bob, bob_before, &carol);
    carol
        .remove_from_private_channel(&channel, &dave.my_id())
        .unwrap();
    sync_all(&everyone).await;

    for member in [&bob, &carol, &dave] {
        assert!(!members_of(member, &channel).contains(&dave.my_id()));
    }
    assert!(listed(&dave, &channel).is_some_and(|listed| !listed.is_member));
    let still_writing = dave.send_message(&channel, "STILL-WRITING", None);
    sync_all(&everyone).await;
    assert!(
        !texts(&bob, &channel).contains(&"STILL-WRITING".to_string()),
        "Dave wrote after his removal: {still_writing:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hand_over_undone_by_a_workspace_removal_leaves_devices_agreeing() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    admin.remove_member(&bob.my_id()).await.unwrap();
    // Bob hands the channel over live, before Carol hears of his removal;
    // the removal then undoes it. Carol acts as owner meanwhile.
    let bob_before = own_last_seq(&bob);
    bob.hand_over_private_channel(&channel, &carol.my_id()).unwrap();
    deliver(&bob, bob_before, &carol);
    carol.rotate_private_channel_key(&channel).unwrap();
    carol.send_message(&channel, "after rotate", None).unwrap();
    for _ in 0..2 {
        for member in [&carol, &admin, &dave] {
            member.sync().await;
        }
    }

    assert_eq!(texts(&carol, &channel), texts(&dave, &channel));
    assert_eq!(owner(&carol, &channel), owner(&dave, &channel));
}

#[tokio::test(flavor = "multi_thread")]
async fn starting_a_channel_twice_ends_the_same_on_every_device() {
    let (_root, _bucket, _admin, bob, carol, dave) = four_members().await;
    let [first_key, second_key] = [new_channel_key(), new_channel_key()];
    let invitees: Vec<Invitee> = [&carol, &dave]
        .iter()
        .map(|member| Invitee {
            member: member.me(),
            after_seq: own_last_seq(member),
        })
        .collect();
    // The id commits to the first start; the second claims the same channel.
    let channel = private_channel_id(&bob.me(), &commitment(&first_key), "first", &invitees).unwrap();
    let [first, second] = [first_key, second_key].map(|key| StoredKey {
        channel: channel.clone(),
        generation: 1,
        key,
    });

    // Bob starts his channel twice with different keys, handing them out in
    // a different order to Carol and Dave so each opens a different start first.
    let mut forger = Forger::new(&bob);
    let ops = [
        forger.seal(forged_grant(&bob, &carol, &channel, 1, first.key)),
        forged_create(&mut forger, &first, "first", &[&carol, &dave]),
        forger.seal(forged_grant(&bob, &dave, &channel, 1, second.key)),
        forged_create(&mut forger, &second, "second", &[&carol, &dave]),
        forger.seal(forged_grant(&bob, &dave, &channel, 1, first.key)),
        forger.seal(forged_grant(&bob, &carol, &channel, 1, second.key)),
    ];
    for envelope in ops {
        carol.receive(envelope.clone()).unwrap();
        dave.receive(envelope).unwrap();
    }

    let name = |member: &Workspace| listed(member, &channel).map(|listed| listed.name);
    assert_eq!(name(&carol), name(&dave));
}

#[tokio::test(flavor = "multi_thread")]
async fn two_owner_ops_for_one_position_end_the_same_everywhere() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    // Bob writes position 2 twice: first an add sealed with a key that only
    // the second (a new key generation) announces, so devices open them in
    // the opposite order to his log.
    let gen1 = newest_key(&bob, &channel);
    let gen2 = StoredKey {
        channel: channel.clone(),
        generation: 2,
        key: new_channel_key(),
    };
    let mut forger = Forger::new(&bob);
    let mut ops = Vec::new();
    for member in [&carol, &dave] {
        ops.push(forger.seal(forged_grant(&bob, member, &channel, 2, gen2.key)));
    }
    ops.push(forger.seal_private(
        &gen2,
        PrivateOp::Add {
            invitee: crate::private::ops::Invitee {
                member: admin.me(),
                after_seq: own_last_seq(&admin),
            },
            from_generation: 1,
            position: 2,
        },
    ));
    ops.push(forger.seal_private(
        &gen1,
        PrivateOp::Rotate {
            generation: 2,
            commitment: commitment(&gen2.key),
            position: 2,
        },
    ));
    for envelope in ops {
        carol.receive(envelope.clone()).unwrap();
        dave.receive(envelope).unwrap();
    }

    for member in [&carol, &dave] {
        let store = &member.state().store;
        assert_eq!(store.latest_generation(&channel).unwrap(), 2);
        assert_eq!(store.next_position(&channel).unwrap(), 3);
    }
    assert_eq!(members_of(&carol, &channel), members_of(&dave, &channel));
}

#[tokio::test(flavor = "multi_thread")]
async fn keys_that_contradict_the_owners_announcement_are_not_kept() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    let mut forger = Forger::new(&carol);
    for _ in 0..3 {
        dave.receive(forger.seal(forged_grant(&carol, &dave, &channel, 1, new_channel_key())))
            .unwrap();
    }
    assert_eq!(dave.state().store.channel_keys(Some(&channel)).unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn nobody_can_plant_a_key_in_someone_s_notes_before_they_open_them() {
    let (_root, _bucket, admin, bob, _carol, _dave) = four_members().await;
    let notes = crate::private::ids::dm_id(&bob.inner.config.workspace_id, &[bob.me()]);
    // The admin wraps a key for Bob's notes to Bob, hoping he'll use it.
    let planted = new_channel_key();
    let granting = Granting {
        workspace_id: &admin.inner.config.workspace_id,
        granter: &admin.me(),
    };
    let bobs_sealing_key = admin.state().store.sealing_key_of(&bob.me()).unwrap().unwrap();
    let grant = GrantBody {
        channel: notes.clone(),
        generation: 0,
        key: planted,
        dm_members: vec![bob.me()],
    };
    let mut forger = Forger::new(&admin);
    bob.receive(forger.seal(Op::KeyGrant {
        wraps: vec![wrap_key(granting, &bobs_sealing_key, &postcard::to_stdvec(&grant).unwrap()).unwrap()],
    }))
    .unwrap();
    bob.receive(forger.seal_private(
        &StoredKey {
            channel: notes.clone(),
            generation: 0,
            key: planted,
        },
        PrivateOp::Content(message(&admin, &notes, "planted")),
    ))
    .unwrap();

    assert_eq!(bob.open_notes().unwrap(), notes);
    bob.send_message(&notes, "my secret", None).unwrap();

    let keys = bob.state().store.channel_keys(Some(&notes)).unwrap();
    assert!(
        keys.iter().all(|key| key.key != planted),
        "Bob's notes never use the planted key"
    );
    assert_eq!(texts(&bob, &notes), ["my secret"]);
}

/// A grant from `granter` to `recipient` alone of `key` for the DM of `members`.
fn forged_dm_grant(granter: &Workspace, recipient: &Workspace, members: &[&Workspace], key: [u8; 32]) -> Op {
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
    let dm_members: Vec<_> = members.iter().map(|member| member.me()).collect();
    let grant = GrantBody {
        channel: dm_id(&granter.inner.config.workspace_id, &dm_members),
        generation: 0,
        key,
        dm_members,
    };
    Op::KeyGrant {
        wraps: vec![wrap_key(granting, &public, &postcard::to_stdvec(&grant).unwrap()).unwrap()],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_cannot_split_a_group_dm_with_a_key_for_each_of_the_others() {
    let (_root, _bucket, _admin, bob, carol, dave) = four_members().await;
    let everyone = [&bob, &carol, &dave];
    let dm = dm_id(
        &dave.inner.config.workspace_id,
        &everyone.map(|member| member.me()),
    );

    // Dave starts the DM by giving Bob one key and Carol another; each
    // device gets both grants but can open only its own.
    let mut forger = Forger::new(&dave);
    let grants = [
        forger.seal(forged_dm_grant(&dave, &bob, &everyone, new_channel_key())),
        forger.seal(forged_dm_grant(&dave, &carol, &everyone, new_channel_key())),
    ];
    for grant in grants {
        bob.receive(grant.clone()).unwrap();
        carol.receive(grant).unwrap();
    }

    let (bob_before, carol_before) = (own_last_seq(&bob), own_last_seq(&carol));
    bob.send_message(&dm, "hi carol", None).unwrap();
    deliver(&bob, bob_before, &carol);
    carol.send_message(&dm, "hi bob", None).unwrap();
    deliver(&carol, carol_before, &bob);
    assert_eq!(texts(&bob, &dm), ["hi carol", "hi bob"]);
    assert_eq!(texts(&carol, &dm), ["hi carol", "hi bob"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_handed_to_one_member_never_carries_what_they_write() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let dm = bob.open_dm(&[carol.my_id(), dave.my_id()]).unwrap();
    bob.send_message(&dm, "first", None).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    // Dave gives Bob alone a key that sorts after every other.
    let mut forger = Forger::new(&dave);
    let grant = forger.seal(forged_dm_grant(&dave, &bob, &[&bob, &carol, &dave], [0xFF; 32]));
    bob.receive(grant.clone()).unwrap();
    carol.receive(grant).unwrap();

    let bob_before = own_last_seq(&bob);
    bob.send_message(&dm, "second", None).unwrap();
    deliver(&bob, bob_before, &carol);
    assert_eq!(texts(&carol, &dm), ["first", "second"]);
}

fn invited(member: &Workspace, channel: &str) -> Vec<String> {
    member.private_members(channel).unwrap().invited
}

fn message_id(op: &Op) -> String {
    let Op::Message { id, .. } = op else {
        panic!("not a message");
    };
    id.clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_listed_without_the_key_shows_as_invited() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    // Bob lists Carol and Dave but gives the key to Dave alone.
    let key = new_channel_key();
    let invitees: Vec<Invitee> = [&carol, &dave]
        .iter()
        .map(|member| Invitee {
            member: member.me(),
            after_seq: own_last_seq(member),
        })
        .collect();
    let channel = private_channel_id(&bob.me(), &commitment(&key), "plans", &invitees).unwrap();
    let first = StoredKey {
        channel: channel.clone(),
        generation: 1,
        key,
    };
    let hello = message(&bob, &channel, "hello");
    let mut forger = Forger::new(&bob);
    let ops = [
        forger.seal(forged_grant(&bob, &dave, &channel, 1, key)),
        forged_create(&mut forger, &first, "plans", &[&carol, &dave]),
        forger.seal_private(&first, PrivateOp::Content(hello.clone())),
    ];
    for envelope in ops {
        carol.receive(envelope.clone()).unwrap();
        dave.receive(envelope).unwrap();
    }

    dave.mark_read(&channel, &message_id(&hello)).unwrap();
    sync_all(&[&admin, &carol, &dave]).await;
    assert_eq!(members_of(&dave, &channel).len(), 3);
    assert_eq!(invited(&dave, &channel), [carol.my_id()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_joins_by_writing_or_by_opening_the_channel() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    assert_eq!(invited(&bob, &channel), [carol.my_id(), dave.my_id()]);

    let hello = carol.send_message(&channel, "hello", None).unwrap();
    sync_all(&[&bob, &carol, &dave]).await;
    assert_eq!(invited(&bob, &channel), [dave.my_id()], "writing counts");
    let dave_before = own_last_seq(&dave);
    sync_all(&[&dave]).await;
    assert_eq!(own_last_seq(&dave), dave_before, "being listed writes nothing");

    dave.mark_read(&channel, &hello).unwrap();
    sync_all(&[&bob, &dave]).await;
    assert!(invited(&bob, &channel).is_empty(), "opening it counts");
    let dave_joined = own_last_seq(&dave);
    sync_all(&[&dave]).await;
    let context = dave.apply_context();
    dave.state()
        .store
        .atomically(|store| store.rebuild_views(context))
        .unwrap();
    sync_all(&[&bob, &dave]).await;
    assert_eq!(own_last_seq(&dave), dave_joined, "the join is written once");
    assert!(invited(&dave, &channel).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn old_ops_never_prove_a_later_membership() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    carol.send_message(&channel, "before", None).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    assert!(!invited(&dave, &channel).contains(&carol.my_id()));
    bob.remove_from_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    // Bob lists Carol again as if from before her old ops, without a key.
    let newest = newest_key(&bob, &channel);
    let position = bob.state().store.next_position(&channel).unwrap();
    let add = Forger::new(&bob).seal_private(
        &newest,
        PrivateOp::Add {
            invitee: Invitee {
                member: carol.me(),
                after_seq: 0,
            },
            from_generation: newest.generation,
            position,
        },
    );
    dave.receive(add).unwrap();
    assert!(members_of(&dave, &channel).contains(&carol.my_id()));
    assert!(invited(&dave, &channel).contains(&carol.my_id()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_left_out_of_a_new_key_is_told() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    assert!(!listed(&carol, &channel).unwrap().is_missing_key);

    // Bob starts generation 2 and gives it to Dave alone.
    let current = newest_key(&bob, &channel);
    let next = new_channel_key();
    let position = bob.state().store.next_position(&channel).unwrap();
    let mut forger = Forger::new(&bob);
    let ops = [
        forger.seal(forged_grant(&bob, &dave, &channel, 2, next)),
        forger.seal_private(
            &current,
            PrivateOp::Rotate {
                generation: 2,
                commitment: commitment(&next),
                position,
            },
        ),
    ];
    for envelope in ops {
        carol.receive(envelope.clone()).unwrap();
        dave.receive(envelope).unwrap();
    }
    assert!(listed(&carol, &channel).unwrap().is_missing_key);
    assert!(!listed(&dave, &channel).unwrap().is_missing_key);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_junk_key_under_the_new_number_doesnt_hide_a_missed_key() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;

    // Bob hands Carol a made-up key labelled generation 2, then starts the
    // real generation 2 for Dave alone.
    let current = newest_key(&bob, &channel);
    let real = new_channel_key();
    let position = bob.state().store.next_position(&channel).unwrap();
    let mut forger = Forger::new(&bob);
    let ops = [
        forger.seal(forged_grant(&bob, &carol, &channel, 2, new_channel_key())),
        forger.seal(forged_grant(&bob, &dave, &channel, 2, real)),
        forger.seal_private(
            &current,
            PrivateOp::Rotate {
                generation: 2,
                commitment: commitment(&real),
                position,
            },
        ),
    ];
    for envelope in ops {
        carol.receive(envelope).unwrap();
    }
    assert!(listed(&carol, &channel).unwrap().is_missing_key);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_re_add_alone_never_makes_a_device_write() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    let first = bob.send_message(&channel, "first", None).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    carol.mark_read(&channel, &first).unwrap();
    sync_all(&[&bob, &carol]).await;
    assert!(!invited(&bob, &channel).contains(&carol.my_id()));

    bob.remove_from_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&[&bob, &carol]).await;
    bob.add_to_private_channel(&channel, &carol.my_id()).unwrap();
    let carol_before = own_last_seq(&carol);
    sync_all(&[&bob, &carol]).await;
    assert_eq!(
        own_last_seq(&carol),
        carol_before,
        "her old read position doesn't count"
    );
    assert!(invited(&bob, &channel).contains(&carol.my_id()));

    let second = bob.send_message(&channel, "second", None).unwrap();
    sync_all(&[&bob, &carol]).await;
    carol.mark_read(&channel, &second).unwrap();
    sync_all(&[&bob, &carol]).await;
    assert!(!invited(&bob, &channel).contains(&carol.my_id()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_join_counts_the_same_in_any_order_and_twice() {
    let (_root, _bucket, _admin, bob, carol, dave) = four_members().await;
    let (bob_before, carol_before) = (own_last_seq(&bob), own_last_seq(&carol));
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), dave.my_id()])
        .unwrap();
    let hello = bob.send_message(&channel, "hello", None).unwrap();
    deliver(&bob, bob_before, &carol);
    carol.mark_read(&channel, &hello).unwrap();
    sync_all(&[&carol]).await;
    assert!(
        own_last_seq(&carol) > carol_before,
        "Carol's device wrote that she joined"
    );

    // Dave hears Carol's join before the channel exists, then all of it again.
    deliver(&carol, carol_before, &dave);
    deliver(&bob, bob_before, &dave);
    deliver(&carol, carol_before, &dave);
    assert_eq!(invited(&dave, &channel), [dave.my_id()]);
}

/// After `poison` from `author`, their next public message still reaches
/// `reader`, and the poison leaves no sync error behind.
async fn later_ops_still_arrive(
    author: &Workspace,
    reader: &Workspace,
    poison: Envelope,
    forger: &mut Forger<'_>,
) {
    let general = channel_id(reader, "general");
    let later = forger.seal(Op::Message {
        id: crate::ops::new_owned_id(&author.me()),
        channel: general.clone(),
        text: "written after the odd op".into(),
        reply_to: None,
    });
    reader.receive(poison).unwrap();
    reader.receive(later).unwrap();
    assert!(texts(reader, &general).contains(&"written after the odd op".to_string()));
    reader.sync().await;
    assert_eq!(reader.status().error, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_grant_with_a_huge_generation_does_not_stall_its_author() {
    let (_root, _bucket, admin, bob, _carol, _dave) = four_members().await;
    let channel = format!("p:{}", "ab".repeat(32));
    let mut forger = Forger::new(&bob);
    let poison = forger.seal(forged_grant(&bob, &admin, &channel, u64::MAX, [9; 32]));
    later_ops_still_arrive(&bob, &admin, poison, &mut forger).await;
}

/// Anyone can start a private channel, so a creation naming a member with
/// an impossible position in their log is the easiest way in.
#[tokio::test(flavor = "multi_thread")]
async fn a_creation_with_a_huge_number_does_not_stall_its_author() {
    for is_key_first in [true, false] {
        let (_root, _bucket, _admin, bob, carol, _dave) = four_members().await;
        let key = new_channel_key();
        let members = vec![Invitee {
            member: carol.me(),
            after_seq: u64::MAX,
        }];
        let channel = private_channel_id(&bob.me(), &commitment(&key), "plans", &members).unwrap();
        let first = StoredKey {
            channel: channel.clone(),
            generation: 1,
            key,
        };
        let creation = PrivateOp::Create {
            name: "plans".into(),
            commitment: commitment(&key),
            members,
        };
        let mut forger = Forger::new(&bob);
        let (earlier, later) = if is_key_first {
            let grant = forger.seal(forged_grant(&bob, &carol, &channel, 1, key));
            (grant, forger.seal_private(&first, creation))
        } else {
            let created = forger.seal_private(&first, creation);
            (created, forger.seal(forged_grant(&bob, &carol, &channel, 1, key)))
        };
        carol.receive(earlier).unwrap();
        later_ops_still_arrive(&bob, &carol, later, &mut forger).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_owner_op_with_a_huge_number_does_not_stall_its_author() {
    let (_root, _bucket, admin, bob, carol, dave) = four_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    let key = newest_key(&bob, &channel);
    let huge_add = PrivateOp::Add {
        invitee: Invitee {
            member: dave.me(),
            after_seq: u64::MAX,
        },
        from_generation: 1,
        position: 2,
    };
    let huge_remove = PrivateOp::Remove {
        member: carol.me(),
        last_seq: u64::MAX,
        generation: 2,
        commitment: commitment(&new_channel_key()),
        position: 2,
    };
    let mut forger = Forger::new(&bob);
    for op in [huge_add, huge_remove] {
        let poison = forger.seal_private(&key, op);
        later_ops_still_arrive(&bob, &carol, poison, &mut forger).await;
    }
}

/// A private channel's name follows the same rule as any channel's: one
/// created with a name that breaks it (punctuation the rule doesn't allow,
/// invisible characters) is skipped, even by its members.
#[tokio::test(flavor = "multi_thread")]
async fn private_channels_created_with_names_that_break_the_rule_are_skipped() {
    let (_root, _bucket, admin, bob, _carol, _dave) = four_members().await;
    let mut forger = Forger::new(&bob);
    for (name, is_shown) in [("q&a", false), ("\u{202e}evil", false), ("fine-name", true)] {
        let key = new_channel_key();
        let invitees = [Invitee {
            member: admin.me(),
            after_seq: own_last_seq(&admin),
        }];
        let channel = private_channel_id(&bob.me(), &commitment(&key), name, &invitees).unwrap();
        let first = StoredKey {
            channel: channel.clone(),
            generation: 1,
            key,
        };
        admin
            .receive(forger.seal(forged_grant(&bob, &admin, &channel, 1, key)))
            .unwrap();
        admin
            .receive(forged_create(&mut forger, &first, name, &[&admin]))
            .unwrap();

        assert_eq!(listed(&admin, &channel).is_some(), is_shown, "{name}");
    }
}
