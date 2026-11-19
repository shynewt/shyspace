//! Invites: single use, approved by the admin, expiring, revocable.

use std::time::Duration;

use super::support::*;
use crate::views::InviteState;

fn invite_state(admin: &crate::Workspace, token: &str) -> InviteState {
    admin
        .invites()
        .unwrap()
        .into_iter()
        .find(|invite| invite.token == token)
        .unwrap()
        .state
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_waits_for_the_admin_to_approve_it() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let invite = admin
        .create_test_invite("for Bob", Duration::from_secs(600))
        .unwrap();
    let token = invite.token.clone();
    let joining = start_join(invite, &root.path().join("bob"), &bucket, "Bob", false);

    wait_for_request(&admin, &token).await;
    let request = admin
        .invites()
        .unwrap()
        .into_iter()
        .find(|invite| invite.token == token)
        .unwrap();
    assert_eq!(request.label, "for Bob");
    assert_eq!(request.requester_name.as_deref(), Some("Bob"));
    for _ in 0..5 {
        admin.sync().await;
    }
    assert!(!joining.is_finished(), "nobody gets in without approval");
    assert_eq!(
        admin
            .members()
            .unwrap()
            .iter()
            .filter(|member| member.active)
            .count(),
        1
    );

    admin.approve_invite(&token).await.unwrap();
    let bob = joining.await.unwrap().unwrap();
    assert_eq!(invite_state(&admin, &token), InviteState::Accepted);
    sync_all(&[&admin, &bob]).await;
    assert!(
        admin
            .members()
            .unwrap()
            .iter()
            .any(|member| member.name == "Bob" && member.active)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_requester_is_told_so() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let invite = admin.create_test_invite("", Duration::from_secs(600)).unwrap();
    let token = invite.token.clone();
    let joining = start_join(invite, &root.path().join("mallory"), &bucket, "Mallory", false);
    wait_for_request(&admin, &token).await;

    admin.reject_invite(&token).await.unwrap();
    let error = joining.await.unwrap().err().unwrap();
    assert!(error.to_string().contains("declined"), "{error}");
    assert_eq!(invite_state(&admin, &token), InviteState::Rejected);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invite_works_only_once() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let invite = admin.create_test_invite("", Duration::from_secs(3)).unwrap();
    let token = invite.token.clone();
    let first = start_join(invite.clone(), &root.path().join("bob"), &bucket, "Bob", false);
    wait_for_request(&admin, &token).await;
    admin.approve_invite(&token).await.unwrap();
    first.await.unwrap().unwrap();

    let second = start_join(invite, &root.path().join("eve"), &bucket, "Eve", false);
    while !second.is_finished() {
        admin.sync().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        second.await.unwrap().is_err(),
        "a used invite must not let a second person in"
    );
    assert!(!admin.members().unwrap().iter().any(|member| member.name == "Eve"));
}

#[tokio::test(flavor = "multi_thread")]
async fn revoked_and_expired_invites_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;

    let revoked = admin.create_test_invite("", Duration::from_secs(600)).unwrap();
    admin.revoke_invite(&revoked.token).await.unwrap();
    let _waiting = start_join(revoked.clone(), &root.path().join("bob"), &bucket, "Bob", false);
    for _ in 0..10 {
        admin.sync().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(invite_state(&admin, &revoked.token), InviteState::Revoked);
    assert!(admin.approve_invite(&revoked.token).await.is_err());

    let expired = admin.create_test_invite("", Duration::from_millis(1)).unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let refused = start_join(
        expired.clone(),
        &root.path().join("carol"),
        &bucket,
        "Carol",
        false,
    );
    let error = refused.await.unwrap().err().unwrap();
    assert!(error.to_string().contains("expired"), "{error}");
    assert_eq!(invite_state(&admin, &expired.token), InviteState::Expired);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_admin_manages_invites() {
    let (_root, _bucket, _admin, bob) = admin_and_bob().await;
    assert!(bob.create_test_invite("", Duration::from_secs(600)).is_err());
    assert!(bob.invites().is_err());
}
