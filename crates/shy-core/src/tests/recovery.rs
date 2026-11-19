//! Restoring a lost admin from their recovery kit.

use super::support::*;
use crate::{Networking, Workspace};

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_admin_is_restored_from_the_recovery_kit() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let general = channel_id(&admin, "general");
    admin
        .send_message(&general, "written before the device was lost", None)
        .unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let admin_id = admin.me();
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let restored = Workspace::restore_with(&new_device, &kit, bucket.clone(), Networking::Offline)
        .await
        .unwrap();
    assert_eq!(restored.me(), admin_id);
    assert!(restored.is_admin());
    let texts: Vec<_> = restored
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(texts.contains(&"written before the device was lost".to_string()));
    let active: Vec<_> = restored
        .members()
        .unwrap()
        .into_iter()
        .filter(|m| m.active)
        .map(|m| m.name)
        .collect();
    assert!(active.contains(&"Bob".to_string()) && active.contains(&"Carol".to_string()));

    // Still the admin: it can change membership, and members follow.
    let dave = join(&restored, &root.path().join("dave"), &bucket, "Dave", false).await;
    restored
        .send_message(&general, "back on a new laptop", None)
        .unwrap();
    sync_all(&[&restored, &bob, &dave]).await;
    let bob_sees: Vec<_> = bob
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(bob_sees.contains(&"back on a new laptop".to_string()));
    assert!(
        bob.members()
            .unwrap()
            .iter()
            .any(|member| member.name == "Dave" && member.active)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forged_or_unknown_kit_is_refused() {
    let (root, bucket, admin, _bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let mut forged = kit.to_string();
    forged.truncate(forged.len() - 4);
    forged.push_str("AAAA");
    let attempt = Workspace::restore_with(
        &data_dir(&root.path().join("x")),
        &forged,
        bucket.clone(),
        Networking::Offline,
    )
    .await;
    assert!(attempt.is_err());

    let fresh_bucket = memory_bucket();
    let attempt = Workspace::restore_with(
        &data_dir(&root.path().join("y")),
        &kit,
        fresh_bucket,
        Networking::Offline,
    )
    .await;
    assert!(attempt.is_err(), "no snapshot in that bucket");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_admin_has_a_recovery_kit() {
    let (_root, _bucket, _admin, bob) = admin_and_bob().await;
    assert!(bob.recovery_kit().await.is_err());
}
