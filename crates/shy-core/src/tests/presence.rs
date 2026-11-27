//! Status (set by hand, synced) and presence (automatic, live only).

use super::support::*;
use crate::workspace::NextBeat;
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
        alerts: crate::views::NotifyLevel::Mentions,
    })
    .unwrap();
    sync_all(&[&admin, &bob]).await;

    let members = admin.members().unwrap();
    let status = member(&members, &bob.my_id()).status.clone().unwrap();
    assert_eq!(
        (status.emoji.as_str(), status.text.as_str(), status.alerts),
        ("🎧", "Deep work", crate::views::NotifyLevel::Mentions)
    );

    bob.set_status(StatusUpdate {
        emoji: "🌴",
        text: "Gone",
        until_ms: Some(now_ms().unwrap() - 1),
        alerts: crate::views::NotifyLevel::All,
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
    assert_eq!(sent(admin.next_heartbeat().unwrap()).state, PresenceState::Away);
    admin.set_appear_offline(true).unwrap();
    assert!(matches!(admin.next_heartbeat().unwrap(), NextBeat::Silent));
    drop(admin);

    let reopened =
        crate::Workspace::open_with(&data_dir(&dir), connector(&bucket), crate::Networking::Offline)
            .await
            .unwrap();
    assert!(reopened.appear_offline().unwrap());
    assert!(
        matches!(reopened.next_heartbeat().unwrap(), NextBeat::Silent),
        "no heartbeat at launch, not even one"
    );
    assert_eq!(
        member(&reopened.members().unwrap(), &reopened.my_id()).presence,
        PresenceState::Unreachable
    );

    reopened.set_appear_offline(false).unwrap();
    assert_eq!(
        sent(reopened.next_heartbeat().unwrap()).state,
        PresenceState::Online
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn heartbeats_go_out_at_most_every_few_seconds_unless_something_changed() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    sent(admin.next_heartbeat().unwrap());
    assert!(
        matches!(admin.next_heartbeat().unwrap(), NextBeat::After(wait) if wait.as_millis() <= 5000),
        "a peer reconnecting again and again doesn't make us beat each time, only a little later"
    );
    admin.set_away(true);
    let beat = sent(admin.next_heartbeat().unwrap());
    assert_eq!(
        beat.state,
        PresenceState::Away,
        "a change of our own goes out at once"
    );
}

fn sent(beat: NextBeat) -> crate::workspace::Heartbeat {
    match beat {
        NextBeat::Now(heartbeat) => heartbeat,
        NextBeat::After(_) | NextBeat::Silent => panic!("expected a heartbeat to go out now"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_is_online_if_any_of_their_devices_is() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;

    admin
        .receive_heartbeat(&phone.heartbeat(PresenceState::Online).unwrap())
        .unwrap();
    assert_eq!(
        member(&admin.members().unwrap(), &bob.my_id()).presence,
        PresenceState::Online,
        "online on the phone only"
    );

    admin
        .receive_heartbeat(&bob.heartbeat(PresenceState::Away).unwrap())
        .unwrap();
    assert_eq!(
        member(&admin.members().unwrap(), &bob.my_id()).presence,
        PresenceState::Online,
        "the best of the two"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_device_still_heard_online_does_not_keep_its_person_online() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    admin
        .receive_heartbeat(&phone.heartbeat(PresenceState::Online).unwrap())
        .unwrap();
    admin
        .receive_heartbeat(&bob.heartbeat(PresenceState::Away).unwrap())
        .unwrap();

    admin.remove_devices(&[phone.my_device_id()]).await.unwrap();

    assert_eq!(
        member(&admin.members().unwrap(), &bob.my_id()).presence,
        PresenceState::Away
    );
}
