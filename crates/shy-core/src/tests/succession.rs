//! A private channel whose owner is removed from the workspace passes to the
//! member its owner added earliest, the same on every device.

use super::support::*;
use crate::{Workspace, private::ops::PrivateOp, storage::Ending, transport::bucket::Bucket};

/// Admin, Bob, Carol and Dave, all synced. Bob owns a private channel that
/// he made with Carol and then added Dave to, so Carol was added first.
async fn bobs_channel() -> (tempfile::TempDir, Bucket, [Workspace; 4], String) {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    bob.add_to_private_channel(&channel, &dave.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    (root, bucket, [admin, bob, carol, dave], channel)
}

fn owner_seen_by(member: &Workspace, channel: &str) -> Option<iroh::PublicKey> {
    member.state().store.current_owner(channel).unwrap()
}

fn members_seen_by(member: &Workspace, channel: &str) -> Vec<iroh::PublicKey> {
    member
        .state()
        .store
        .private_members(channel, Ending::Left)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn when_the_owner_is_removed_the_member_added_first_takes_over() {
    let (_root, _bucket, [admin, bob, carol, dave], channel) = bobs_channel().await;

    admin.remove_member(&bob.my_id()).await.unwrap();
    sync_all(&[&admin, &carol, &dave]).await;

    for member in [&carol, &dave] {
        assert_eq!(owner_seen_by(member, &channel), Some(carol.me()));
        assert_eq!(members_seen_by(member, &channel), [carol.me(), dave.me()]);
    }
    carol.add_to_private_channel(&channel, &admin.my_id()).unwrap();
    sync_all(&[&admin, &carol, &dave]).await;
    assert!(
        admin
            .channels()
            .unwrap()
            .iter()
            .any(|listed| listed.id == channel)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_next_member_takes_over_when_the_first_in_line_is_removed_too() {
    let (_root, _bucket, [admin, bob, carol, dave], channel) = bobs_channel().await;

    admin.remove_member(&bob.my_id()).await.unwrap();
    admin.remove_member(&carol.my_id()).await.unwrap();
    sync_all(&[&admin, &dave]).await;

    assert_eq!(owner_seen_by(&dave, &channel), Some(dave.me()));
    assert_eq!(members_seen_by(&dave, &channel), [dave.me()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_successor_who_had_left_passes_the_channel_on() {
    let (_root, _bucket, [admin, bob, carol, dave], channel) = bobs_channel().await;
    carol.leave_private_channel(&channel).unwrap();
    // Bob is offline, so he never removes her as owners do.
    sync_all(&[&admin, &carol, &dave]).await;

    admin.remove_member(&bob.my_id()).await.unwrap();
    sync_all(&[&admin, &carol, &dave]).await;
    sync_all(&[&admin, &carol, &dave]).await;

    for member in [&carol, &dave] {
        assert_eq!(owner_seen_by(member, &channel), Some(dave.me()));
    }
    assert_eq!(members_seen_by(&dave, &channel), [dave.me()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rebuild_keeps_the_successor() {
    let (_root, _bucket, [admin, bob, carol, dave], channel) = bobs_channel().await;
    admin.remove_member(&bob.my_id()).await.unwrap();
    sync_all(&[&admin, &carol, &dave]).await;
    let before = (owner_seen_by(&dave, &channel), members_seen_by(&dave, &channel));

    dave.state().store.rebuild_views(dave.apply_context()).unwrap();

    assert_eq!(
        (owner_seen_by(&dave, &channel), members_seen_by(&dave, &channel)),
        before
    );
    assert_eq!(before.0, Some(carol.me()));
}

#[tokio::test(flavor = "multi_thread")]
async fn nobody_takes_over_while_the_owner_is_still_in_the_workspace() {
    let (_root, _bucket, [admin, bob, carol, dave], channel) = bobs_channel().await;

    sync_all(&[&admin, &bob, &carol, &dave]).await;

    assert_eq!(owner_seen_by(&carol, &channel), Some(bob.me()));
    assert!(carol.add_to_private_channel(&channel, &admin.my_id()).is_err());
}

fn newest_key(member: &Workspace, channel: &str) -> crate::storage::StoredKey {
    member
        .state()
        .store
        .channel_keys(Some(channel))
        .unwrap()
        .pop()
        .unwrap()
}

fn next_position(member: &Workspace, channel: &str) -> u64 {
    member.state().store.next_position(channel).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claim_from_someone_not_next_in_line_never_counts() {
    let (_root, _bucket, [admin, bob, carol, dave], channel) = bobs_channel().await;
    let position = next_position(&carol, &channel);
    admin.remove_member(&bob.my_id()).await.unwrap();
    sync_all(&[&admin, &dave]).await;
    // Written after the removal, so only the order of succession stops it.
    let jumping_the_queue =
        Forger::new(&dave).seal_private(&newest_key(&dave, &channel), PrivateOp::Succeed { position });

    carol.receive(jumping_the_queue).unwrap();
    sync_all(&[&admin, &carol]).await;

    assert_eq!(owner_seen_by(&carol, &channel), Some(carol.me()));
    assert!(
        carol.state().store.waiting_in(&channel).unwrap().is_empty(),
        "the claim is settled as void"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claim_planted_before_the_owner_was_removed_never_counts() {
    let (_root, _bucket, [admin, bob, carol, dave], channel) = bobs_channel().await;
    // Carol, first in line, plants a claim and a delete of the whole channel
    // while Bob still owns it. Both are in her log when she is removed.
    let position = next_position(&carol, &channel);
    let key = newest_key(&carol, &channel);
    let mut forger = Forger::new(&carol);
    let planted = [
        forger.seal_private(&key, PrivateOp::Succeed { position }),
        forger.seal_private(
            &key,
            PrivateOp::OwnerDelete {
                id: channel.clone(),
                position: position + 1,
            },
        ),
    ];
    for envelope in &planted {
        admin.receive(envelope.clone()).unwrap();
        dave.receive(envelope.clone()).unwrap();
    }

    admin.remove_member(&carol.my_id()).await.unwrap();
    admin.remove_member(&bob.my_id()).await.unwrap();
    sync_all(&[&admin, &dave]).await;

    assert_eq!(owner_seen_by(&dave, &channel), Some(dave.me()));
    assert!(dave.channels().unwrap().iter().any(|listed| listed.id == channel));
}

/// What `member` wrote since their log stood at `before`.
fn written_since(member: &Workspace, before: u64) -> Vec<crate::ops::Envelope> {
    member
        .state()
        .store
        .envelopes_after(&member.me(), before)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claim_waits_until_the_removed_owner_s_last_ops_arrive() {
    let (_root, _bucket, [admin, bob, carol, dave], channel) = bobs_channel().await;
    let before = bob.state().store.last_seq(&bob.me()).unwrap();
    bob.send_message(&channel, "last words", None).unwrap();
    // Sent live to Ada and Carol only, never to the bucket, so it counts in
    // Bob's log (he is removed after it) but Dave doesn't have it yet.
    let last_words = written_since(&bob, before);
    for envelope in &last_words {
        admin.receive(envelope.clone()).unwrap();
        carol.receive(envelope.clone()).unwrap();
    }
    admin.remove_member(&bob.my_id()).await.unwrap();
    sync_all(&[&admin, &carol]).await;
    sync_all(&[&admin, &carol, &dave]).await;
    assert_eq!(
        owner_seen_by(&dave, &channel),
        Some(bob.me()),
        "Bob's log isn't complete on Dave"
    );

    for envelope in last_words {
        dave.receive(envelope).unwrap();
    }

    assert_eq!(owner_seen_by(&dave, &channel), Some(carol.me()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rebuild_lets_the_removed_owner_s_last_change_beat_a_claim_for_its_position() {
    let (_root, _bucket, [admin, bob, carol, dave], _) = bobs_channel().await;
    // Carol made this channel and handed it to Bob, so in a rebuild her own
    // ops bring the owner log right up to Bob's last change.
    let channel = carol.create_private_channel("ops", &[bob.my_id()]).unwrap();
    carol.add_to_private_channel(&channel, &dave.my_id()).unwrap();
    carol.hand_over_private_channel(&channel, &bob.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    let bobs_last_position = next_position(&bob, &channel);
    bob.add_to_private_channel(&channel, &admin.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    admin.remove_member(&bob.my_id()).await.unwrap();
    sync_all(&[&admin, &carol, &dave]).await;
    let live = (owner_seen_by(&dave, &channel), members_seen_by(&dave, &channel));
    assert_eq!(live.0, Some(carol.me()));
    // Carol, first in line, also claims the position Bob's add took, under
    // the key in use then, so a rebuild can judge it right at that position.
    let key_then = carol
        .state()
        .store
        .channel_keys(Some(&channel))
        .unwrap()
        .remove(0);
    dave.receive(Forger::new(&carol).seal_private(
        &key_then,
        PrivateOp::Succeed {
            position: bobs_last_position,
        },
    ))
    .unwrap();

    dave.state().store.rebuild_views(dave.apply_context()).unwrap();

    assert_eq!(
        (owner_seen_by(&dave, &channel), members_seen_by(&dave, &channel)),
        live
    );
    assert!(live.1.contains(&admin.me()), "Bob's add still counts");
}

#[tokio::test(flavor = "multi_thread")]
async fn someone_the_owner_removed_from_the_channel_cannot_take_over() {
    let (_root, _bucket, [admin, bob, carol, dave], channel) = bobs_channel().await;
    bob.remove_from_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol, &dave]).await;
    let position = next_position(&dave, &channel);
    admin.remove_member(&bob.my_id()).await.unwrap();
    sync_all(&[&admin, &carol]).await;
    let claim =
        Forger::new(&carol).seal_private(&newest_key(&carol, &channel), PrivateOp::Succeed { position });

    dave.receive(claim).unwrap();
    sync_all(&[&admin, &dave]).await;

    assert_eq!(owner_seen_by(&dave, &channel), Some(dave.me()));
}
