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
