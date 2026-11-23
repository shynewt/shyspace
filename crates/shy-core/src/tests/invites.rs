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

/// A name the app would refuse fails the join before the admin is asked, so
/// the invite isn't spent and no half-made workspace is left behind; trying
/// again with a good name works.
#[tokio::test(flavor = "multi_thread")]
async fn a_name_the_app_refuses_fails_the_join_before_asking_the_admin() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let invite = admin
        .create_test_invite("for Bob", Duration::from_secs(600))
        .unwrap();
    let token = invite.token.clone();
    let bob_dir = root.path().join("bob");

    let joining = start_join(invite.clone(), &bob_dir, &bucket, &"B".repeat(200), false);
    let refused = tokio::time::timeout(Duration::from_secs(10), joining)
        .await
        .expect("refused right away, not after waiting for the admin")
        .unwrap();

    assert!(refused.is_err());
    assert!(!bob_dir.exists(), "nothing was made");
    let unnamed = start_join(invite.clone(), &bob_dir, &bucket, "  ", false)
        .await
        .unwrap();
    assert!(unnamed.is_err(), "an empty name the admin would drop");
    admin.sync().await;
    let state = admin
        .invites()
        .unwrap()
        .into_iter()
        .find(|listed| listed.token == token)
        .unwrap()
        .state;
    assert_eq!(state, InviteState::Open, "the admin wasn't asked");

    let again = start_join(invite, &bob_dir, &bucket, "Bob", false);
    wait_for_request(&admin, &token).await;
    admin.approve_invite(&token).await.unwrap();
    assert!(again.await.unwrap().is_ok());
}

/// Once the admin's answer is stored, a join is finished, not undone: a name
/// that didn't get written then is written when the workspace next opens.
#[tokio::test(flavor = "multi_thread")]
async fn a_join_cut_short_after_the_answer_finishes_on_the_next_open() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob_dir = root.path().join("bob");
    let bob = join(&admin, &bob_dir, &bucket, "Bob", false).await;
    assert_eq!(
        admin.state().store.meta("pending_profile").unwrap(),
        None,
        "a create finishes its own"
    );
    // As if the app stopped before the chosen name was sent.
    let chosen = serde_json::json!({ "name": "Bobby", "is_bot": false }).to_string();
    bob.state()
        .store
        .set_meta("pending_profile", chosen.as_bytes())
        .unwrap();
    bob.shutdown().await.unwrap();
    drop(bob);

    let reopened = crate::Workspace::open_with(
        &data_dir(&bob_dir),
        connector(&bucket),
        crate::Networking::Offline,
    )
    .await
    .unwrap();
    sync_all(&[&admin, &reopened]).await;

    assert!(
        admin
            .members()
            .unwrap()
            .iter()
            .any(|member| member.name == "Bobby")
    );
    assert_eq!(reopened.state().store.meta("pending_profile").unwrap(), None);
}

/// An answer goes up once; the admin doesn't put it again on every pass
/// while the joiner hasn't synced yet (only now and then, in case someone
/// with the bucket keys removed it).
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_goes_up_once_not_on_every_pass() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let invite = admin
        .create_test_invite("for Bob", Duration::from_secs(600))
        .unwrap();
    let token = invite.token.clone();
    let joining = start_join(invite, &root.path().join("bob"), &bucket, "Bob", false);
    wait_for_request(&admin, &token).await;
    admin.approve_invite(&token).await.unwrap();
    let bob = joining.await.unwrap().unwrap();
    bucket.delete_invite_answer(&token).await.unwrap();

    admin.sync().await;
    assert_eq!(
        bucket.invite_answer(&token).await.unwrap(),
        None,
        "not put again this soon"
    );

    sync_all(&[&bob, &admin]).await;
    assert!(
        admin
            .members()
            .unwrap()
            .iter()
            .any(|member| member.name == "Bob" && member.active)
    );
}

/// A create cut short after its folder is complete still gives the admin
/// their name on the next open.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_cut_short_finishes_the_admins_name_on_the_next_open() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let ada_dir = root.path().join("ada");
    let admin = create_admin(&ada_dir, &bucket).await;
    // As if the app stopped before the chosen name was written.
    let chosen = serde_json::json!({ "name": "Ada Lovelace", "is_bot": false }).to_string();
    admin
        .state()
        .store
        .set_meta("pending_profile", chosen.as_bytes())
        .unwrap();
    admin.shutdown().await.unwrap();
    drop(admin);

    let reopened = crate::Workspace::open_with(
        &data_dir(&ada_dir),
        connector(&bucket),
        crate::Networking::Offline,
    )
    .await
    .unwrap();

    assert!(
        reopened
            .members()
            .unwrap()
            .iter()
            .any(|member| member.name == "Ada Lovelace")
    );
}
