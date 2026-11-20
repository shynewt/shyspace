//! Invites: single use, approved by the admin, expiring, revocable.

use std::time::Duration;

use super::support::*;
use crate::{JoinCancel, views::InviteState};

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

#[tokio::test(flavor = "multi_thread")]
async fn the_admin_sees_the_safety_code_the_joiner_sees() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob_dir = root.path().join("bob");
    let bob_code = data_dir(&bob_dir).safety_code().await.unwrap();
    let invite = admin.create_test_invite("", Duration::from_secs(600)).unwrap();
    let token = invite.token.clone();
    let joining = start_join(invite, &bob_dir, &bucket, "Bob", false);
    wait_for_request(&admin, &token).await;
    let request = admin
        .invites()
        .unwrap()
        .into_iter()
        .find(|invite| invite.token == token)
        .unwrap();
    assert_eq!(request.requester_safety_code.as_deref(), Some(bob_code.as_str()));
    admin.approve_invite(&token).await.unwrap();
    let bob = joining.await.unwrap().unwrap();
    assert_eq!(bob.safety_code(), bob_code);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admin_shows_a_newcomer_by_their_requested_name_right_away() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let _claude = join(&admin, &root.path().join("claude"), &bucket, "Claude", true).await;
    // The newcomer's own profile hasn't reached the admin yet.
    let newcomer = admin
        .members()
        .unwrap()
        .into_iter()
        .find(|member| !member.is_admin)
        .unwrap();
    assert_eq!(newcomer.name, "Claude");
    assert!(newcomer.is_bot);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_join_can_be_cancelled_while_it_waits_and_tried_again() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let invite = admin
        .create_test_invite("for Bob", Duration::from_secs(600))
        .unwrap();
    let token = invite.token.clone();
    let bob_dir = root.path().join("bob");
    let cancel = JoinCancel::new();
    let first = start_cancellable_join(invite.clone(), &bob_dir, &bucket, "Bob", false, cancel.clone());
    wait_for_request(&admin, &token).await;

    cancel.cancel();

    let stopped = tokio::time::timeout(Duration::from_secs(1), first)
        .await
        .expect("stops right away, not at the next poll or the invite's expiry")
        .unwrap();
    let Err(error) = stopped else {
        panic!("a cancelled join must not finish")
    };
    assert!(error.to_string().contains("cancelled"), "{error:#}");
    // The admin still has the first request, which this device can still answer.
    let again = start_join(invite, &bob_dir, &bucket, "Bob", false);
    admin.approve_invite(&token).await.unwrap();
    let bob = again.await.unwrap().unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(
        admin
            .members()
            .unwrap()
            .iter()
            .any(|member| member.name == "Bob" && member.active)
    );
}
