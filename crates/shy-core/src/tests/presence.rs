//! Status (set by hand, synced) and presence (automatic, live only).

use super::support::*;
use crate::{
    ops::now_ms,
    views::{PresenceState, StatusUpdate},
};

fn member<'a>(members: &'a [crate::views::Member], id: &str) -> &'a crate::views::Member {
    members.iter().find(|member| member.id == id).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_status_reaches_everyone_and_expires() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let in_an_hour = now_ms().unwrap() + 3_600_000;
    bob.set_status(StatusUpdate {
        emoji: "🎧",
        text: "Deep work",
        until_ms: Some(in_an_hour),
        is_busy: true,
    })
    .unwrap();
    sync_all(&[&admin, &bob]).await;

    let members = admin.members().unwrap();
    let status = member(&members, &bob.my_id()).status.clone().unwrap();
    assert_eq!(
        (status.emoji.as_str(), status.text.as_str(), status.is_busy),
        ("🎧", "Deep work", true)
    );

    bob.set_status(StatusUpdate {
        emoji: "🌴",
        text: "Gone",
        until_ms: Some(now_ms().unwrap() - 1),
        is_busy: false,
    })
    .unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(
        member(&admin.members().unwrap(), &bob.my_id()).status.is_none(),
        "an expired status is gone"
    );

    bob.clear_status().unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(member(&admin.members().unwrap(), &bob.my_id()).status.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn presence_follows_heartbeats() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    assert_eq!(
        member(&admin.members().unwrap(), &bob.my_id()).presence,
        PresenceState::Unreachable
    );

    admin
        .receive_heartbeat(&bob.heartbeat(PresenceState::Away).unwrap())
        .unwrap();
    admin
        .receive_heartbeat(&carol.heartbeat(PresenceState::Online).unwrap())
        .unwrap();
    let members = admin.members().unwrap();
    assert_eq!(member(&members, &bob.my_id()).presence, PresenceState::Away);
    assert_eq!(member(&members, &carol.my_id()).presence, PresenceState::Online);
}

#[tokio::test(flavor = "multi_thread")]
async fn appearing_offline_is_remembered_before_anything_is_sent() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("ada");
    let bucket = memory_bucket();
    let admin = create_admin(&dir, &bucket).await;
    admin.set_away(true);
    assert_eq!(
        admin.next_heartbeat().unwrap().unwrap().state,
        PresenceState::Away
    );
    admin.set_appear_offline(true).unwrap();
    assert!(admin.next_heartbeat().unwrap().is_none());
    drop(admin);

    let reopened =
        crate::Workspace::open_with(&data_dir(&dir), connector(&bucket), crate::Networking::Offline)
            .await
            .unwrap();
    assert!(reopened.appear_offline().unwrap());
    assert!(
        reopened.next_heartbeat().unwrap().is_none(),
        "no heartbeat at launch, not even one"
    );
    assert_eq!(
        member(&reopened.members().unwrap(), &reopened.my_id()).presence,
        PresenceState::Unreachable
    );

    reopened.set_appear_offline(false).unwrap();
    assert_eq!(
        reopened.next_heartbeat().unwrap().unwrap().state,
        PresenceState::Online
    );
}
