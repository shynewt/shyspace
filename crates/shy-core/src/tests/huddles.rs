//! Voice huddles between members: who sees whom, who hears whom, and the
//! rules a hostile member can't get around. Beacons travel by hand here, as
//! the members are offline; `Via` says how they'd have arrived.

use std::{sync::Arc, time::Duration};

use super::support::*;
use crate::{
    Networking, Workspace, WorkspaceOptions,
    huddle::{
        beacon::{Beacon, BeaconBody, BeaconState, MEDIA_WIRE, Reach},
        media::{MediaEvent, MicState, fake::FakeMedia},
        rules::MAX_PARTICIPANTS,
    },
    ops::now_ms,
    views::member_id,
    workspace::Via,
};

fn joined_state() -> BeaconState {
    BeaconState::Joined {
        camera: false,
        screen: false,
        reach: Reach::Direct,
    }
}

fn with_media(media: &FakeMedia) -> WorkspaceOptions {
    WorkspaceOptions::from(Networking::Offline).with_media(Arc::new(media.clone()))
}

struct Member {
    workspace: Workspace,
    media: FakeMedia,
}

/// Ada (the admin), Bob and Carol, each with fake audio.
async fn three_members() -> (tempfile::TempDir, [Member; 3]) {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let ada_media = FakeMedia::default();
    let ada = create_admin_with(&root.path().join("ada"), &bucket, with_media(&ada_media)).await;
    let mut others = Vec::new();
    for name in ["Bob", "Carol"] {
        let media = FakeMedia::default();
        let workspace = join_with(&ada, &root.path().join(name), &bucket, name, with_media(&media)).await;
        others.push(Member { workspace, media });
    }
    let [bob, carol] = <[Member; 2]>::try_from(others).ok().unwrap();
    sync_all(&[&ada, &bob.workspace, &carol.workspace]).await;
    sync_all(&[&ada, &bob.workspace, &carol.workspace]).await;
    (
        root,
        [
            Member {
                workspace: ada,
                media: ada_media,
            },
            bob,
            carol,
        ],
    )
}

/// Hands everything `from` sent to each of `to`, as `via` would.
fn carry(from: &Workspace, to: &[&Workspace], via: Via) {
    for beacon in from.take_sent_beacons() {
        for member in to {
            member.receive_beacon(&beacon, via).unwrap();
        }
    }
}

fn in_huddle(member: &Workspace, channel: &str) -> Vec<String> {
    member
        .huddle(channel)
        .unwrap()
        .participants
        .into_iter()
        .map(|participant| participant.member)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn members_join_hear_each_other_and_leave() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");

    ada.workspace.join_huddle(&general, false).unwrap();
    carry(&ada.workspace, &[&bob.workspace, &carol.workspace], Via::Gossip);
    assert_eq!(in_huddle(&bob.workspace, &general), [ada.workspace.my_id()]);
    assert!(ada.media.peers().is_empty(), "alone in the huddle");

    bob.workspace.join_huddle(&general, false).unwrap();
    carry(&bob.workspace, &[&ada.workspace, &carol.workspace], Via::Gossip);
    let bob_path = bob.media.my_path().unwrap();
    assert_eq!(
        ada.media.peers().into_iter().collect::<Vec<_>>(),
        [(bob.workspace.me(), bob_path)]
    );
    assert_eq!(
        bob.media.peers().keys().copied().collect::<Vec<_>>(),
        [ada.workspace.me()]
    );
    assert_eq!(in_huddle(&carol.workspace, &general).len(), 2);
    assert!(carol.media.peers().is_empty(), "watching isn't hearing");

    bob.workspace.leave_huddle().unwrap();
    carry(&bob.workspace, &[&ada.workspace, &carol.workspace], Via::Gossip);
    assert!(ada.media.peers().is_empty());
    assert_eq!(in_huddle(&carol.workspace, &general), [ada.workspace.my_id()]);
    assert!(bob.workspace.my_huddle().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn every_join_publishes_at_a_new_path() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    let first = ada.media.my_path().unwrap();
    ada.workspace.leave_huddle().unwrap();
    ada.workspace.join_huddle(&general, false).unwrap();
    assert_ne!(ada.media.my_path().unwrap(), first);
}

#[tokio::test(flavor = "multi_thread")]
async fn muting_closes_the_microphone() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, true).unwrap();
    assert!(ada.media.is_muted());
    assert_eq!(ada.workspace.my_huddle().unwrap().mic, MicState::Off);
    ada.workspace.set_huddle_muted(false).unwrap();
    assert!(!ada.media.is_muted());
    ada.media.report(MediaEvent::Mic(MicState::Live)).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while ada.workspace.my_huddle().unwrap().mic != MicState::Live {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_member_is_no_longer_heard_or_seen() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    bob.workspace.join_huddle(&general, false).unwrap();
    carry(&ada.workspace, &[&bob.workspace, &carol.workspace], Via::Gossip);
    carry(&bob.workspace, &[&ada.workspace, &carol.workspace], Via::Gossip);
    assert_eq!(ada.media.peers().len(), 1);

    ada.workspace.remove_member(&bob.workspace.my_id()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !ada.media.peers().is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the removed member was still offered audio");
    assert_eq!(in_huddle(&ada.workspace, &general), [ada.workspace.my_id()]);

    // Bob, who hasn't heard yet, joins afresh; nobody takes it.
    bob.workspace.join_huddle(&general, false).unwrap();
    let rejoin = bob.workspace.take_sent_beacons().pop().unwrap();
    assert_eq!(rejoin.body.state, joined_state());
    let error = ada.workspace.receive_beacon(&rejoin, Via::Gossip).unwrap_err();
    assert!(format!("{error:#}").contains("may not be in this huddle"));
    assert!(ada.media.peers().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_private_huddle_stays_among_its_members() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let plans = ada
        .workspace
        .create_private_channel("plans", &[bob.workspace.my_id()])
        .unwrap();
    sync_all(&[&ada.workspace, &bob.workspace, &carol.workspace]).await;

    ada.workspace.join_huddle(&plans, false).unwrap();
    let beacon = ada.workspace.take_sent_beacons().pop().unwrap();
    assert!(
        bob.workspace.receive_beacon(&beacon, Via::Gossip).is_err(),
        "a private huddle's beacon must not travel over gossip"
    );
    assert!(
        bob.workspace
            .receive_beacon(&beacon, Via::Direct(carol.workspace.me()))
            .is_err(),
        "relayed by a device other than its sender"
    );
    bob.workspace
        .receive_beacon(&beacon, Via::Direct(ada.workspace.me()))
        .unwrap();
    assert_eq!(in_huddle(&bob.workspace, &plans), [ada.workspace.my_id()]);

    assert!(
        carol.workspace.huddle(&plans).is_err(),
        "an outsider can't look in"
    );
    assert!(carol.workspace.join_huddle(&plans, false).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_outsiders_beacon_doesnt_get_them_heard() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let plans = ada
        .workspace
        .create_private_channel("plans", &[bob.workspace.my_id()])
        .unwrap();
    sync_all(&[&ada.workspace, &bob.workspace, &carol.workspace]).await;
    ada.workspace.join_huddle(&plans, false).unwrap();

    // Carol's own app could sign a beacon for a channel she isn't in.
    let carol_inner = &carol.workspace.inner;
    let forged = Beacon::sign(
        &carol_inner.identity,
        &carol_inner.config.workspace_id,
        BeaconBody {
            member: carol.workspace.me(),
            endpoint: carol.workspace.me(),
            channel: plans.clone(),
            session: [9; 16],
            seq: 0,
            sent_ms: now_ms().unwrap(),
            state: joined_state(),
            media_wire: MEDIA_WIRE,
        },
    )
    .unwrap();
    let error = ada
        .workspace
        .receive_beacon(&forged, Via::Direct(carol.workspace.me()))
        .unwrap_err();
    assert!(format!("{error:#}").contains("may not be in this huddle"));
    assert!(ada.media.peers().is_empty());
    assert_eq!(in_huddle(&bob.workspace, &plans), Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dm_huddle_is_for_its_two_members() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let dm = ada.workspace.open_dm(&[bob.workspace.my_id()]).unwrap();
    sync_all(&[&ada.workspace, &bob.workspace, &carol.workspace]).await;
    ada.workspace.join_huddle(&dm, false).unwrap();
    carry(&ada.workspace, &[&bob.workspace], Via::Direct(ada.workspace.me()));
    bob.workspace.join_huddle(&dm, false).unwrap();
    carry(&bob.workspace, &[&ada.workspace], Via::Direct(bob.workspace.me()));
    assert_eq!(
        ada.media.peers().keys().copied().collect::<Vec<_>>(),
        [bob.workspace.me()]
    );
    assert!(carol.workspace.huddle(&dm).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_left_session_cant_be_replayed_back_in() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    let joined = ada.workspace.take_sent_beacons();
    ada.workspace.leave_huddle().unwrap();
    let left = ada.workspace.take_sent_beacons();
    assert!(left.iter().all(|beacon| beacon.body.state == BeaconState::Left));
    for beacon in joined.iter().chain(&left) {
        bob.workspace.receive_beacon(beacon, Via::Gossip).unwrap();
    }
    assert!(in_huddle(&bob.workspace, &general).is_empty());
    let replay: &Beacon = &joined[0];
    assert!(bob.workspace.receive_beacon(replay, Via::Gossip).is_err());
    assert!(in_huddle(&bob.workspace, &general).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn without_audio_a_member_sees_huddles_but_cant_join() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let ada = create_admin(&root.path().join("ada"), &bucket).await;
    let general = channel_id(&ada, "general");
    assert!(ada.huddle(&general).unwrap().participants.is_empty());
    let error = ada.join_huddle(&general, false).unwrap_err();
    assert!(format!("{error:#}").contains("no audio"));
}

#[tokio::test(flavor = "multi_thread")]
async fn speaking_is_reported_only_for_people_you_hear() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    bob.workspace.join_huddle(&general, false).unwrap();
    carry(&bob.workspace, &[&ada.workspace], Via::Gossip);
    let mut changes = ada.workspace.subscribe();
    ada.media
        .report(MediaEvent::Speaking {
            peer: carol.workspace.me(),
            speaking: true,
        })
        .await;
    ada.media
        .report(MediaEvent::Speaking {
            peer: bob.workspace.me(),
            speaking: true,
        })
        .await;
    let first = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(crate::views::Change::HuddleSpeaking { member, speaking }) = changes.recv().await {
                return (member, speaking);
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(first, (member_id(&bob.workspace.me()), true));
}

async fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting until {what}"));
}

#[tokio::test(flavor = "multi_thread")]
async fn removed_from_a_private_channel_you_leave_its_huddle() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let plans = ada
        .workspace
        .create_private_channel("plans", &[bob.workspace.my_id()])
        .unwrap();
    sync_all(&[&ada.workspace, &bob.workspace, &carol.workspace]).await;
    bob.workspace.join_huddle(&plans, false).unwrap();
    bob.workspace.take_sent_beacons();

    ada.workspace
        .remove_from_private_channel(&plans, &bob.workspace.my_id())
        .unwrap();
    sync_all(&[&ada.workspace, &bob.workspace]).await;
    wait_until("Bob left", || bob.workspace.my_huddle().is_none()).await;
    assert!(bob.media.my_path().is_none(), "the audio stopped");
    wait_until("Bob said so", || {
        bob.workspace
            .take_sent_beacons()
            .iter()
            .any(|beacon| beacon.body.state == BeaconState::Left)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_the_workspace_ends_the_call() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    ada.workspace.shutdown().await.unwrap();
    wait_until("the audio stopped", || ada.media.my_path().is_none()).await;
    assert!(ada.workspace.my_huddle().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn joining_twice_at_once_leaves_no_ghost() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    let design = ada.workspace.create_channel("design").unwrap();
    // Two threads at once, as two clicks reaching the app together would.
    let joins = [general.clone(), design.clone()].map(|channel| {
        let workspace = ada.workspace.clone();
        tokio::task::spawn_blocking(move || workspace.join_huddle(&channel, false))
    });
    for join in joins {
        join.await.unwrap().unwrap();
    }
    let current = ada.workspace.my_huddle().unwrap();
    let sent = ada.workspace.take_sent_beacons();
    for beacon in sent
        .iter()
        .filter(|beacon| beacon.body.state != BeaconState::Left)
    {
        let is_current =
            beacon.body.channel == current.channel && Some(beacon.media_path_name()) == ada.media.my_path();
        let was_left = sent
            .iter()
            .any(|other| other.body.session == beacon.body.session && other.body.state == BeaconState::Left);
        assert!(is_current || was_left, "a session was announced and never ended");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_huddle_takes_nobody_else() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let ada_media = FakeMedia::default();
    let ada = create_admin_with(&root.path().join("ada"), &bucket, with_media(&ada_media)).await;
    let mut others = Vec::new();
    for index in 0..MAX_PARTICIPANTS {
        let name = format!("Member {index}");
        let media = FakeMedia::default();
        others.push(join_with(&ada, &root.path().join(&name), &bucket, &name, with_media(&media)).await);
    }
    let everyone: Vec<&Workspace> = std::iter::once(&ada).chain(&others).collect();
    sync_all(&everyone).await;
    let general = channel_id(&ada, "general");
    for member in &others {
        member.join_huddle(&general, true).unwrap();
        carry(member, &[&ada], Via::Gossip);
    }
    assert!(ada.huddle(&general).unwrap().is_full);
    let error = ada.join_huddle(&general, true).unwrap_err();
    assert!(format!("{error:#}").contains("full"));
    assert!(ada_media.my_path().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn racing_joins_and_leaves_keep_audio_and_state_in_step() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    let design = ada.workspace.create_channel("design").unwrap();
    for _ in 0..20 {
        let calls = [Some(general.clone()), None, Some(design.clone())].map(|channel| {
            let workspace = ada.workspace.clone();
            tokio::task::spawn_blocking(move || match channel {
                Some(channel) => workspace.join_huddle(&channel, false),
                None => workspace.leave_huddle(),
            })
        });
        for call in calls {
            if let Err(error) = call.await.unwrap() {
                panic!("joins and leaves take turns, so none should fail: {error:#}");
            }
        }
        let state_says = ada.workspace.my_huddle().map(|mine| mine.channel);
        let sent = ada.workspace.take_sent_beacons();
        let current_path = ada.media.my_path();
        assert_eq!(
            state_says.is_some(),
            current_path.is_some(),
            "audio and state disagree"
        );
        if let Some(path) = current_path {
            let announced = sent.iter().rev().find(|beacon| beacon.media_path_name() == path);
            assert!(
                announced.is_some_and(|beacon| beacon.body.state != BeaconState::Left),
                "the running audio belongs to a session that was ended or never announced"
            );
        }
        ada.workspace.leave_huddle().unwrap();
        ada.workspace.take_sent_beacons();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn joining_and_leaving_work_from_a_plain_thread() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    let workspace = ada.workspace.clone();
    // A thread outside the async runtime, like a UI framework's main thread.
    std::thread::spawn(move || {
        workspace.join_huddle(&general, false).unwrap();
        workspace.leave_huddle().unwrap();
    })
    .join()
    .unwrap();
}
