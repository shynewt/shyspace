//! Voice huddles between members: who sees whom, who hears whom, and the
//! rules a hostile member can't get around. Beacons travel by hand here, as
//! the members are offline; `Via` says how they'd have arrived.

use std::{sync::Arc, time::Duration};

use super::support::*;
use crate::{
    Networking, Workspace, WorkspaceOptions,
    huddle::{
        beacon::{Beacon, BeaconBody, BeaconState, MEDIA_WIRE, Reach},
        media::{DeviceChoice, Link, MediaEvent, MicState, fake::FakeMedia},
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

pub(super) struct Member {
    pub(super) workspace: Workspace,
    pub(super) media: FakeMedia,
}

/// Ada (the admin), Bob and Carol, each with fake audio.
pub(super) async fn three_members() -> (tempfile::TempDir, [Member; 3]) {
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
pub(super) fn carry(from: &Workspace, to: &[&Workspace], via: Via) {
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
async fn a_public_huddles_beacon_only_travels_over_gossip() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");

    ada.workspace.join_huddle(&general, false).unwrap();
    let beacon = ada.workspace.take_sent_beacons().pop().unwrap();
    let error = bob
        .workspace
        .receive_beacon(&beacon, Via::Direct(ada.workspace.me()))
        .unwrap_err();
    assert!(format!("{error:#}").contains("arrived directly"), "{error:#}");
    assert!(in_huddle(&bob.workspace, &general).is_empty());

    bob.workspace.receive_beacon(&beacon, Via::Gossip).unwrap();
    assert_eq!(in_huddle(&bob.workspace, &general), [ada.workspace.my_id()]);
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
async fn copying_someones_session_doesnt_get_their_audio_path() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    bob.workspace.join_huddle(&general, false).unwrap();
    ada.workspace.join_huddle(&general, false).unwrap();
    let adas = ada.workspace.take_sent_beacons();
    for beacon in &adas {
        bob.workspace.receive_beacon(beacon, Via::Gossip).unwrap();
    }

    // Carol's own app copies Ada's session id from her beacon into Carol's.
    let carol_inner = &carol.workspace.inner;
    let copied = Beacon::sign(
        &carol_inner.identity,
        &carol_inner.config.workspace_id,
        BeaconBody {
            member: carol.workspace.me(),
            endpoint: carol.workspace.me(),
            channel: general.clone(),
            session: adas[0].body.session,
            seq: 0,
            sent_ms: now_ms().unwrap(),
            state: joined_state(),
            media_wire: MEDIA_WIRE,
        },
    )
    .unwrap();
    bob.workspace.receive_beacon(&copied, Via::Gossip).unwrap();

    let heard = bob.media.peers();
    let adas_path = &heard[&ada.workspace.me()];
    assert_eq!(Some(adas_path.clone()), ada.media.my_path());
    assert_ne!(
        &heard[&carol.workspace.me()],
        adas_path,
        "Carol got Ada's audio path by copying her session"
    );
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

fn huddles_started(
    changes: &mut tokio::sync::broadcast::Receiver<crate::views::Change>,
) -> Vec<(String, String)> {
    let mut started = Vec::new();
    while let Ok(change) = changes.try_recv() {
        if let crate::views::Change::HuddleStarted { channel, member, .. } = change {
            started.push((channel, member));
        }
    }
    started
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dm_huddle_starting_alerts_the_others_once() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let dm = ada.workspace.open_dm(&[bob.workspace.my_id()]).unwrap();
    let general = channel_id(&ada.workspace, "general");
    sync_all(&[&ada.workspace, &bob.workspace, &carol.workspace]).await;
    let mut changes = bob.workspace.subscribe();

    ada.workspace.join_huddle(&general, false).unwrap();
    carry(&ada.workspace, &[&bob.workspace], Via::Gossip);
    assert!(
        huddles_started(&mut changes).is_empty(),
        "public huddles don't ring"
    );

    ada.workspace.join_huddle(&dm, false).unwrap();
    let joined = ada.workspace.take_sent_beacons();
    let ada_joins_dm: Vec<&Beacon> = joined
        .iter()
        .filter(|beacon| beacon.body.channel == dm && beacon.body.state != BeaconState::Left)
        .collect();
    for beacon in &joined {
        let via = if beacon.body.channel == dm {
            Via::Direct(ada.workspace.me())
        } else {
            Via::Gossip
        };
        bob.workspace.receive_beacon(beacon, via).unwrap();
    }
    assert_eq!(
        huddles_started(&mut changes),
        [(dm.clone(), ada.workspace.my_id())]
    );

    // A refresh, or Bob joining too, doesn't ring again.
    let mut refresh = ada_joins_dm[0].clone();
    refresh.body.seq += 1;
    let ada_inner = &ada.workspace.inner;
    let refresh = Beacon::sign(&ada_inner.identity, &ada_inner.config.workspace_id, refresh.body).unwrap();
    bob.workspace
        .receive_beacon(&refresh, Via::Direct(ada.workspace.me()))
        .unwrap();
    assert!(huddles_started(&mut changes).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_muted_dm_doesnt_ring() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = ada.workspace.open_dm(&[bob.workspace.my_id()]).unwrap();
    sync_all(&[&ada.workspace, &bob.workspace]).await;
    bob.workspace
        .set_notify_level(&dm, crate::views::NotifyLevel::Nothing)
        .unwrap();
    let mut changes = bob.workspace.subscribe();
    ada.workspace.join_huddle(&dm, false).unwrap();
    carry(&ada.workspace, &[&bob.workspace], Via::Direct(ada.workspace.me()));
    assert!(huddles_started(&mut changes).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_chosen_devices_are_kept_and_used_for_the_next_call() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let devices = ada.workspace.huddle_devices().unwrap();
    assert_eq!(
        devices.chosen,
        DeviceChoice::default(),
        "the system's default until chosen"
    );

    let mut changes = ada.workspace.subscribe();
    let choice = DeviceChoice {
        input: Some("usb-mic".into()),
        output: Some("headset".into()),
    };
    ada.workspace.set_huddle_devices(choice.clone()).unwrap();
    assert_eq!(ada.workspace.huddle_devices().unwrap().chosen, choice);
    let mut saw_change = false;
    while let Ok(change) = changes.try_recv() {
        saw_change |= matches!(change, crate::views::Change::HuddleDevices);
    }
    assert!(saw_change, "pickers reload when the choice changes");

    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    assert_eq!(
        ada.media.devices(),
        choice,
        "the call starts on the chosen devices"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn joining_never_waits_for_the_device_list() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let gone = DeviceChoice {
        input: Some("a-mic-unplugged-since".into()),
        output: None,
    };
    ada.workspace.set_huddle_devices(gone.clone()).unwrap();
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    assert_eq!(ada.media.listings(), 0, "the join listed the devices");
    assert_eq!(
        ada.media.devices(),
        gone,
        "a device that's gone is passed on, for the media layer to report"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chosen_device_that_went_away_shows_as_the_default() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    ada.workspace
        .set_huddle_devices(DeviceChoice {
            input: Some("a-mic-unplugged-since".into()),
            output: Some("headset".into()),
        })
        .unwrap();
    let chosen = ada.workspace.huddle_devices().unwrap().chosen;
    assert_eq!(chosen.input, None);
    assert_eq!(chosen.output.as_deref(), Some("headset"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_list_change_during_a_call_reloads_the_pickers() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    let mut changes = ada.workspace.subscribe();
    ada.media.report(MediaEvent::DevicesChanged).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(changes.recv().await, Ok(crate::views::Change::HuddleDevices)) {}
    })
    .await
    .expect("no huddle_devices change");
}

/// Ada and Bob, Bob also on a linked phone, each with fake media.
struct TwoDevices {
    _root: tempfile::TempDir,
    ada: Workspace,
    ada_media: FakeMedia,
    bob: Workspace,
    bob_media: FakeMedia,
    phone: Workspace,
    general: String,
}

async fn bob_on_two_devices() -> TwoDevices {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let ada_media = FakeMedia::default();
    let ada = create_admin_with(&root.path().join("ada"), &bucket, with_media(&ada_media)).await;
    let bob_media = FakeMedia::default();
    let bob = join_with(
        &ada,
        &root.path().join("bob"),
        &bucket,
        "Bob",
        with_media(&bob_media),
    )
    .await;
    let phone = link_device_with(
        &ada,
        &bob,
        &root.path().join("phone"),
        &bucket,
        with_media(&FakeMedia::default()),
    )
    .await;
    sync_all(&[&ada, &bob, &phone]).await;
    let general = channel_id(&ada, "general");
    TwoDevices {
        _root: root,
        ada,
        ada_media,
        bob,
        bob_media,
        phone,
        general,
    }
}

/// Everyone joins `channel` and hears everyone else.
fn all_join(members: &[&Workspace], channel: &str) {
    for member in members {
        member.join_huddle(channel, false).unwrap();
    }
    for from in members {
        let others: Vec<&Workspace> = members
            .iter()
            .copied()
            .filter(|to| to.me() != from.me())
            .collect();
        carry(from, &others, Via::Gossip);
    }
}

/// Waits until `media`'s workspace has applied every event reported before:
/// events apply in order, and this one ends with a change we can see.
async fn after_earlier_media_events(member: &Workspace, media: &FakeMedia) {
    let mut changes = member.subscribe();
    media.report(MediaEvent::MeSpeaking(true)).await;
    wait_for_speaking(&mut changes, (&member.my_id(), true)).await;
}

/// The speaking changes before the first that is exactly `until`.
async fn wait_for_speaking(
    changes: &mut tokio::sync::broadcast::Receiver<crate::views::Change>,
    until: (&str, bool),
) -> Vec<(String, bool)> {
    let mut seen = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(crate::views::Change::HuddleSpeaking { member, speaking }) = changes.recv().await {
                if (member.as_str(), speaking) == until {
                    return;
                }
                seen.push((member, speaking));
            }
        }
    })
    .await
    .unwrap();
    seen
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_in_a_huddle_on_two_devices_shows_once() {
    let TwoDevices {
        ada,
        ada_media,
        bob,
        phone,
        general,
        ..
    } = bob_on_two_devices().await;
    all_join(&[&ada, &bob, &phone], &general);

    assert_eq!(in_huddle(&ada, &general), [ada.my_id(), bob.my_id()]);
    assert_eq!(in_huddle(&bob, &general), [bob.my_id(), ada.my_id()]);
    let mine = bob.my_huddle().unwrap();
    assert!(
        mine.peers.iter().all(|peer| peer.member != bob.my_id()),
        "my phone isn't a peer"
    );
    let peers: Vec<String> = ada
        .my_huddle()
        .unwrap()
        .peers
        .into_iter()
        .map(|peer| peer.member)
        .collect();
    assert_eq!(
        peers,
        [bob.my_id()],
        "Bob once, though both his devices are peers"
    );

    let mut changes = ada.subscribe();
    ada_media
        .report(MediaEvent::Speaking {
            peer: phone.me(),
            speaking: true,
        })
        .await;
    let speaking = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(crate::views::Change::HuddleSpeaking { member, speaking }) = changes.recv().await {
                return (member, speaking);
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        speaking,
        (bob.my_id(), true),
        "Bob's phone speaking is Bob speaking"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_device_never_rings_for_a_dm_call() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let ada_media = FakeMedia::default();
    let ada = create_admin_with(&root.path().join("ada"), &bucket, with_media(&ada_media)).await;
    let bob = join(&ada, &root.path().join("bob"), &bucket, "Bob", false).await;
    let phone_media = FakeMedia::default();
    let phone = link_device_with(
        &ada,
        &bob,
        &root.path().join("phone"),
        &bucket,
        with_media(&phone_media),
    )
    .await;
    sync_all(&[&ada, &bob, &phone]).await;
    let dm = ada.open_dm(&[bob.my_id()]).unwrap();
    sync_all(&[&ada, &bob, &phone]).await;

    ada.join_huddle(&dm, false).unwrap();
    // DMs are a person's first device's until they go by person: the phone
    // doesn't have this one.
    for beacon in ada.take_sent_beacons() {
        assert!(phone.receive_beacon(&beacon, Via::Direct(ada.me())).is_err());
    }

    assert_eq!(phone_media.rings_playing(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_persons_devices_merge_into_one_peer_with_the_best_link_and_speaking() {
    let TwoDevices {
        ada,
        ada_media,
        bob,
        phone,
        general,
        ..
    } = bob_on_two_devices().await;
    all_join(&[&ada, &bob, &phone], &general);
    let bob_peer = || {
        let peers = ada.my_huddle().unwrap().peers;
        assert_eq!(peers.len(), 1, "Bob once");
        peers.into_iter().next().unwrap()
    };

    for (laptop_link, phone_link) in [(Link::Relayed, Link::Direct), (Link::Direct, Link::Relayed)] {
        for (peer, link) in [(bob.me(), laptop_link), (phone.me(), phone_link)] {
            ada_media.report(MediaEvent::Link { peer, link }).await;
        }
        after_earlier_media_events(&ada, &ada_media).await;
        assert_eq!(bob_peer().link, Link::Direct, "the best of his two links");
    }
    for (laptop_speaking, phone_speaking) in [(true, false), (false, true)] {
        for (peer, speaking) in [(bob.me(), laptop_speaking), (phone.me(), phone_speaking)] {
            ada_media.report(MediaEvent::Speaking { peer, speaking }).await;
        }
        after_earlier_media_events(&ada, &ada_media).await;
        assert!(bob_peer().speaking, "speaking while either device is");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn my_other_device_adds_nothing_to_my_own_row() {
    let TwoDevices {
        ada,
        bob,
        phone,
        general,
        ..
    } = bob_on_two_devices().await;
    all_join(&[&ada, &bob, &phone], &general);
    // The phone's app says its camera is on.
    let inner = &phone.inner;
    let beacon = Beacon::sign(
        &inner.identity,
        &inner.config.workspace_id,
        BeaconBody {
            member: phone.me(),
            endpoint: phone.me(),
            channel: general.clone(),
            session: [7; 16],
            seq: 0,
            sent_ms: now_ms().unwrap(),
            state: BeaconState::Joined {
                camera: true,
                screen: false,
                reach: Reach::Direct,
            },
            media_wire: MEDIA_WIRE,
        },
    )
    .unwrap();
    bob.receive_beacon(&beacon, Via::Gossip).unwrap();

    let mine = bob.huddle(&general).unwrap();
    let my_rows: Vec<_> = mine
        .participants
        .iter()
        .filter(|row| row.member == bob.my_id())
        .collect();
    assert!(
        my_rows.len() == 1 && my_rows[0].is_me && !my_rows[0].camera,
        "{my_rows:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_speaks_while_any_of_their_devices_does_and_my_own_devices_say_nothing() {
    let TwoDevices {
        ada,
        ada_media,
        bob,
        bob_media,
        phone,
        general,
        ..
    } = bob_on_two_devices().await;
    all_join(&[&ada, &bob, &phone], &general);
    let mut changes = ada.subscribe();

    for (peer, speaking) in [
        (phone.me(), true),
        (bob.me(), true),
        (phone.me(), false),
        (bob.me(), false),
    ] {
        ada_media.report(MediaEvent::Speaking { peer, speaking }).await;
    }
    ada_media.report(MediaEvent::MeSpeaking(true)).await;

    assert_eq!(
        wait_for_speaking(&mut changes, (&ada.my_id(), true)).await,
        [(bob.my_id(), true), (bob.my_id(), false)],
        "Bob starts once and stops once"
    );
    let mut bobs_changes = bob.subscribe();
    bob_media
        .report(MediaEvent::Speaking {
            peer: phone.me(),
            speaking: true,
        })
        .await;
    // Ends on what my phone speaking would wrongly have said first.
    bob_media.report(MediaEvent::MeSpeaking(false)).await;
    assert_eq!(
        wait_for_speaking(&mut bobs_changes, (&bob.my_id(), false)).await,
        Vec::<(String, bool)>::new(),
        "nothing for my own phone"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_linked_during_a_call_shows_as_its_person() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let ada_media = FakeMedia::default();
    let ada = create_admin_with(&root.path().join("ada"), &bucket, with_media(&ada_media)).await;
    let bob = join_with(
        &ada,
        &root.path().join("bob"),
        &bucket,
        "Bob",
        with_media(&FakeMedia::default()),
    )
    .await;
    sync_all(&[&ada, &bob]).await;
    let general = channel_id(&ada, "general");
    all_join(&[&ada, &bob], &general);

    let phone = link_device_with(
        &ada,
        &bob,
        &root.path().join("phone"),
        &bucket,
        with_media(&FakeMedia::default()),
    )
    .await;
    sync_all(&[&ada, &bob, &phone]).await;
    ada.age_huddles(Duration::from_millis(crate::huddle::ring::CUE_GAP_MS));
    let cues_before = ada_media.cues();
    phone.join_huddle(&general, false).unwrap();
    carry(&phone, &[&ada, &bob], Via::Gossip);

    let peers: Vec<String> = ada
        .my_huddle()
        .unwrap()
        .peers
        .into_iter()
        .map(|peer| peer.member)
        .collect();
    assert_eq!(peers, [bob.my_id()]);
    assert_eq!(
        ada_media.cues(),
        cues_before,
        "Bob's phone joining plays no sound"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_huddle_is_full_by_devices_not_people() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let ada = create_admin_with(
        &root.path().join("ada"),
        &bucket,
        with_media(&FakeMedia::default()),
    )
    .await;
    let mut others = Vec::new();
    for index in 0..MAX_PARTICIPANTS - 1 {
        let name = format!("Member {index}");
        others.push(
            join_with(
                &ada,
                &root.path().join(&name),
                &bucket,
                &name,
                with_media(&FakeMedia::default()),
            )
            .await,
        );
    }
    let joined: Vec<&Workspace> = std::iter::once(&ada).chain(&others).collect();
    sync_all(&joined).await;
    let phone = link_device_with(
        &ada,
        &others[0],
        &root.path().join("phone"),
        &bucket,
        with_media(&FakeMedia::default()),
    )
    .await;
    others.push(phone);
    let everyone: Vec<&Workspace> = std::iter::once(&ada).chain(&others).collect();
    sync_all(&everyone).await;
    let general = channel_id(&ada, "general");
    for member in &others {
        member.join_huddle(&general, true).unwrap();
        carry(member, &[&ada], Via::Gossip);
    }

    let huddle = ada.huddle(&general).unwrap();
    assert_eq!(
        huddle.participants.len(),
        MAX_PARTICIPANTS - 1,
        "one person on two devices"
    );
    assert!(huddle.is_full, "eight devices fill it");
}

#[tokio::test(flavor = "multi_thread")]
async fn my_own_other_device_joining_plays_no_sound() {
    let TwoDevices {
        ada,
        bob,
        bob_media,
        phone,
        general,
        ..
    } = bob_on_two_devices().await;
    all_join(&[&ada, &bob], &general);
    bob.age_huddles(Duration::from_millis(crate::huddle::ring::CUE_GAP_MS));
    let cues_before = bob_media.cues();

    phone.join_huddle(&general, false).unwrap();
    carry(&phone, &[&ada, &bob], Via::Gossip);

    assert_eq!(bob_media.cues(), cues_before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_speaking_device_leaving_while_its_person_stays_stops_them_speaking() {
    let TwoDevices {
        ada,
        ada_media,
        bob,
        phone,
        general,
        ..
    } = bob_on_two_devices().await;
    all_join(&[&ada, &bob, &phone], &general);
    ada_media
        .report(MediaEvent::Speaking {
            peer: phone.me(),
            speaking: true,
        })
        .await;
    after_earlier_media_events(&ada, &ada_media).await;
    let mut changes = ada.subscribe();

    phone.leave_huddle().unwrap();
    carry(&phone, &[&ada], Via::Gossip);
    ada_media.report(MediaEvent::MeSpeaking(false)).await;

    assert_eq!(
        wait_for_speaking(&mut changes, (&ada.my_id(), false)).await,
        [(bob.my_id(), false)],
        "Bob stays, on his laptop, and isn't speaking"
    );
}
