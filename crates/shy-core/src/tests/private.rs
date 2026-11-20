//! Private channels and DMs: sealing keys, grants, sealed content.

use super::support::*;
use crate::{Networking, Workspace};

fn newest_sealing_key(member: &Workspace) -> Vec<u8> {
    member.state().store.sealing_secrets().unwrap()[0].public.clone()
}

fn announced_sealing_key(viewer: &Workspace, member: &Workspace) -> Option<Vec<u8>> {
    viewer.state().store.sealing_key_of(&member.me()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn every_member_announces_a_sealing_key_once() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for viewer in [&admin, &bob] {
        for member in [&admin, &bob] {
            assert_eq!(
                announced_sealing_key(viewer, member),
                Some(newest_sealing_key(member))
            );
        }
    }

    let next_seq = bob.state().store.next_seq(&bob.me()).unwrap();
    drop(bob);
    let bob = Workspace::open_with(
        &data_dir(&root.path().join("bob")),
        connector(&bucket),
        Networking::Offline,
    )
    .await
    .unwrap();
    assert_eq!(
        bob.state().store.next_seq(&bob.me()).unwrap(),
        next_seq,
        "a restart announces nothing new"
    );
    assert_eq!(bob.state().store.sealing_secrets().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restored_admin_keeps_its_sealing_key() {
    let (root, bucket, admin, _bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let sealing_key = newest_sealing_key(&admin);
    drop(admin);

    let restored = Workspace::restore_with(
        &data_dir(&root.path().join("ada-new-laptop")),
        &kit,
        connector(&bucket),
        Networking::Offline,
    )
    .await
    .unwrap();
    assert_eq!(newest_sealing_key(&restored), sealing_key);
}

fn texts(member: &Workspace, channel: &str) -> Vec<String> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect()
}

fn lists(member: &Workspace, channel: &str) -> bool {
    member
        .channels()
        .unwrap()
        .iter()
        .any(|listed| listed.id == channel)
}

/// Admin, Bob and Carol, all synced.
async fn three_members() -> (
    tempfile::TempDir,
    crate::transport::bucket::Bucket,
    Workspace,
    Workspace,
    Workspace,
) {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    (root, bucket, admin, bob, carol)
}

#[tokio::test(flavor = "multi_thread")]
async fn only_members_of_a_private_channel_can_read_it() {
    let (_root, _bucket, admin, bob, carol) = three_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    bob.send_message(&channel, "PRIVATE-PLAN", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    for member in [&bob, &carol] {
        assert!(lists(member, &channel));
        assert_eq!(texts(member, &channel), ["PRIVATE-PLAN"]);
    }
    assert!(!lists(&admin, &channel), "the admin isn't in it");
    assert!(texts(&admin, &channel).is_empty());
    assert!(admin.search("PRIVATE-PLAN", 10).unwrap().is_empty());
    assert_eq!(carol.search("PRIVATE-PLAN", 10).unwrap().len(), 1);

    // Everyone still follows everyone's log past the private ops.
    bob.send_message(&channel_id(&admin, "general"), "public after private", None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(texts(&admin, &channel_id(&admin, "general")).contains(&"public after private".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn someone_added_later_reads_the_history() {
    let (_root, _bucket, admin, bob, carol) = three_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    bob.send_message(&channel, "before Ada joined", None).unwrap();
    bob.add_to_private_channel(&channel, &admin.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    assert_eq!(texts(&admin, &channel), ["before Ada joined"]);
    admin.send_message(&channel, "hello from Ada", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert_eq!(texts(&carol, &channel), ["before Ada joined", "hello from Ada"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_member_cannot_read_or_write_what_comes_next() {
    let (_root, _bucket, admin, bob, carol) = three_members().await;
    let channel = bob
        .create_private_channel("plans", &[carol.my_id(), admin.my_id()])
        .unwrap();
    bob.send_message(&channel, "everyone saw this", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    bob.remove_from_private_channel(&channel, &carol.my_id()).unwrap();
    bob.send_message(&channel, "AFTER-CAROL-LEFT", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    assert_eq!(texts(&carol, &channel), ["everyone saw this"]);
    assert_eq!(texts(&admin, &channel), ["everyone saw this", "AFTER-CAROL-LEFT"]);
    assert!(carol.send_message(&channel, "still here?", None).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owner_can_hand_a_channel_over() {
    let (_root, _bucket, admin, bob, carol) = three_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    bob.hand_over_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    assert!(bob.add_to_private_channel(&channel, &admin.my_id()).is_err());
    carol.add_to_private_channel(&channel, &admin.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert!(lists(&admin, &channel));
    bob.leave_private_channel(&channel).unwrap();
    assert!(bob.send_message(&channel, "gone", None).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dm_is_shared_by_exactly_its_members() {
    let (_root, _bucket, admin, bob, carol) = three_members().await;
    let dm = bob.open_dm(&[carol.my_id()]).unwrap();
    assert_eq!(
        bob.open_dm(&[carol.my_id()]).unwrap(),
        dm,
        "opening it again finds it"
    );
    bob.send_message(&dm, "PSST", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    assert_eq!(carol.open_dm(&[bob.my_id()]).unwrap(), dm);
    assert_eq!(texts(&carol, &dm), ["PSST"]);
    carol.send_message(&dm, "hi back", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert_eq!(texts(&bob, &dm), ["PSST", "hi back"]);
    assert!(texts(&admin, &dm).is_empty());
    assert!(!lists(&admin, &dm));
}

#[tokio::test(flavor = "multi_thread")]
async fn replies_reactions_edits_and_deletes_work_inside() {
    let (_root, _bucket, admin, bob, carol) = three_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    let root = bob.send_message(&channel, "root", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let reply = carol.send_message(&channel, "reply", Some(root.clone())).unwrap();
    carol.react(&root, "👍", true).unwrap();
    carol.edit_message(&reply, "reply, edited").unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    let messages = bob.messages(&channel, None, 50).unwrap();
    assert_eq!(messages.len(), 1, "the reply is in the thread");
    assert_eq!(messages[0].reactions.len(), 1);
    assert_eq!(messages[0].reply_count, 1);
    // The owner may delete anyone's message there; others only their own.
    assert!(carol.delete_message(&root).is_err());
    bob.delete_message(&reply).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert_eq!(carol.messages(&channel, None, 50).unwrap()[0].reply_count, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn someone_who_left_misses_what_comes_next_until_added_back() {
    let (_root, _bucket, admin, bob, carol) = three_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    carol.leave_private_channel(&channel).unwrap();
    assert!(carol.send_message(&channel, "gone", None).is_err());
    sync_all(&[&admin, &bob, &carol]).await;
    let generation = bob.state().store.latest_generation(&channel).unwrap();
    assert_eq!(generation, 2, "the owner made a key Carol doesn't have");

    bob.send_message(&channel, "AFTER-CAROL-LEFT", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert!(texts(&carol, &channel).is_empty());

    bob.add_to_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert_eq!(
        texts(&carol, &channel),
        ["AFTER-CAROL-LEFT"],
        "back, with the history"
    );
    carol.send_message(&channel, "back again", None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert_eq!(texts(&bob, &channel), ["AFTER-CAROL-LEFT", "back again"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_someone_from_the_workspace_takes_them_out_of_private_channels() {
    let (_root, _bucket, admin, bob, carol) = three_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    admin.remove_member(&carol.my_id()).await.unwrap();
    sync_all(&[&admin, &bob]).await;

    let store = &bob.state().store;
    assert_eq!(store.latest_generation(&channel).unwrap(), 2);
    assert_eq!(store.current_private_members(&channel).unwrap(), [bob.me()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_channel_list_shows_kind_membership_and_who_may_delete() {
    use crate::views::ChannelKind;
    let (_root, _bucket, admin, bob, carol) = three_members().await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    let dm = bob.open_dm(&[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let find = |member: &Workspace, id: &str| {
        member
            .channels()
            .unwrap()
            .into_iter()
            .find(|listed| listed.id == id)
            .unwrap()
    };

    let seen_by_bob = find(&bob, &channel);
    assert_eq!(seen_by_bob.kind, ChannelKind::Private);
    assert!(seen_by_bob.is_member && seen_by_bob.can_delete);
    assert!(!find(&carol, &channel).can_delete, "only the owner deletes it");
    let dm_for_carol = find(&carol, &dm);
    assert_eq!(dm_for_carol.kind, ChannelKind::Dm);
    assert_eq!(dm_for_carol.dm_members, [bob.my_id()]);

    let members = carol.private_members(&channel).unwrap();
    assert_eq!(members.members, [bob.my_id(), carol.my_id()]);
    assert_eq!(members.owner, Some(bob.my_id()));
    bob.remove_from_private_channel(&channel, &carol.my_id()).unwrap();
    sync_all(&[&bob, &carol]).await;
    assert!(!find(&carol, &channel).is_member);
}
