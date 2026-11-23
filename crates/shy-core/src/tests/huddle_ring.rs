//! The ring for an incoming DM huddle: when it sounds, and everything that
//! stops it.

use std::time::Duration;

use super::{
    huddles::{Member, carry, three_members},
    support::*,
};
use crate::{
    huddle::{
        beacon::Beacon,
        ring::{RING_MS, RING_QUIET_MS},
    },
    views::{Change, NotifyLevel},
    workspace::Via,
};

/// A DM between `caller` and `callee`, both up to date.
async fn dm_between(caller: &Member, callee: &Member) -> String {
    let dm = caller.workspace.open_dm(&[callee.workspace.my_id()]).unwrap();
    sync_all(&[&caller.workspace, &callee.workspace]).await;
    dm
}

/// `caller` starts a huddle in `dm`, and `callee` hears of it.
fn call(caller: &Member, callee: &Member, dm: &str) {
    caller.workspace.join_huddle(dm, false).unwrap();
    carry(
        &caller.workspace,
        &[&callee.workspace],
        Via::Direct(caller.workspace.me()),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_dm_call_rings_until_answered() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    call(&ada, &bob, &dm);
    assert_eq!(bob.media.rings_playing(), 1);

    bob.workspace.join_huddle(&dm, false).unwrap();
    assert_eq!(bob.media.rings_playing(), 0, "answering stops the ring");
}

#[tokio::test(flavor = "multi_thread")]
async fn declining_stops_the_ring_and_says_so() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    call(&ada, &bob, &dm);
    let mut changes = bob.workspace.subscribe();

    bob.workspace.decline_huddle();

    assert_eq!(bob.media.rings_playing(), 0);
    let mut stopped = Vec::new();
    while let Ok(change) = changes.try_recv() {
        if let Change::HuddleRingStopped { channel } = change {
            stopped.push(channel);
        }
    }
    assert_eq!(stopped, [dm]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_caller_hanging_up_stops_the_ring() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    call(&ada, &bob, &dm);

    ada.workspace.leave_huddle().unwrap();
    carry(&ada.workspace, &[&bob.workspace], Via::Direct(ada.workspace.me()));

    assert_eq!(bob.media.rings_playing(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unanswered_ring_stops_after_a_while() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    call(&ada, &bob, &dm);
    bob.workspace.expire_huddle_participants().unwrap();
    assert_eq!(bob.media.rings_playing(), 1, "still ringing early on");

    bob.workspace.age_huddles(Duration::from_millis(RING_MS + 1));
    bob.workspace.expire_huddle_participants().unwrap();

    assert_eq!(bob.media.rings_playing(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_doesnt_ring_someone_already_in_a_huddle() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    let general = channel_id(&bob.workspace, "general");
    bob.workspace.join_huddle(&general, false).unwrap();

    call(&ada, &bob, &dm);

    assert_eq!(bob.media.rings_started(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_muted_dm_doesnt_ring() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    bob.workspace.set_notify_level(&dm, NotifyLevel::Nothing).unwrap();

    call(&ada, &bob, &dm);

    assert_eq!(bob.media.rings_started(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn catching_up_on_a_call_already_going_doesnt_ring() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    ada.workspace.join_huddle(&dm, false).unwrap();
    // Bob first hears of the call from a later refresh, after a restart, say.
    let mut refresh = ada.workspace.take_sent_beacons()[0].clone();
    refresh.body.seq = 3;
    let ada_inner = &ada.workspace.inner;
    let refresh = Beacon::sign(&ada_inner.identity, &ada_inner.config.workspace_id, refresh.body).unwrap();

    bob.workspace
        .receive_beacon(&refresh, Via::Direct(ada.workspace.me()))
        .unwrap();

    assert_eq!(bob.media.rings_started(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_call_replaces_the_first_ring() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let with_ada = dm_between(&ada, &bob).await;
    let with_carol = dm_between(&carol, &bob).await;
    call(&ada, &bob, &with_ada);
    call(&carol, &bob, &with_carol);

    assert_eq!(bob.media.rings_started(), 2);
    assert_eq!(bob.media.rings_playing(), 1, "one ring at a time");

    // Ada hanging up doesn't silence Carol's call.
    ada.workspace.leave_huddle().unwrap();
    carry(&ada.workspace, &[&bob.workspace], Via::Direct(ada.workspace.me()));
    assert_eq!(bob.media.rings_playing(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ring_says_who_is_calling_while_it_lasts() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    let mut changes = bob.workspace.subscribe();
    call(&ada, &bob, &dm);

    let mut ringing = Vec::new();
    while let Ok(change) = changes.try_recv() {
        if let Change::HuddleRinging {
            channel,
            member,
            member_name,
        } = change
        {
            ringing.push((channel, member, member_name));
        }
    }
    assert_eq!(ringing, [(dm.clone(), ada.workspace.my_id(), "Ada".to_owned())]);
    let incoming = bob.workspace.huddle_ringing().expect("ringing");
    assert_eq!((incoming.channel, incoming.member), (dm, ada.workspace.my_id()));

    bob.workspace.decline_huddle();
    assert_eq!(bob.workspace.huddle_ringing(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_that_doesnt_ring_doesnt_say_it_rings() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    bob.workspace.set_notify_level(&dm, NotifyLevel::Nothing).unwrap();
    let mut changes = bob.workspace.subscribe();
    call(&ada, &bob, &dm);

    while let Ok(change) = changes.try_recv() {
        assert!(!matches!(change, Change::HuddleRinging { .. }), "{change:?}");
    }
    assert_eq!(bob.workspace.huddle_ringing(), None);
}

/// How many times `callee` was told of a huddle starting, since `changes` began.
fn alerts(changes: &mut tokio::sync::broadcast::Receiver<Change>) -> usize {
    let mut count = 0;
    while let Ok(change) = changes.try_recv() {
        if matches!(change, Change::HuddleStarted { .. }) {
            count += 1;
        }
    }
    count
}

fn hang_up(caller: &Member, callee: &Member) {
    caller.workspace.leave_huddle().unwrap();
    carry(
        &caller.workspace,
        &[&callee.workspace],
        Via::Direct(caller.workspace.me()),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_caller_cant_ring_again_and_again() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    let mut changes = bob.workspace.subscribe();
    call(&ada, &bob, &dm);
    assert_eq!(bob.media.rings_playing(), 1);
    hang_up(&ada, &bob);

    for _ in 0..3 {
        call(&ada, &bob, &dm);
        assert_eq!(
            bob.media.rings_playing(),
            0,
            "a call right after an unanswered one stays quiet"
        );
        hang_up(&ada, &bob);
    }
    assert_eq!(alerts(&mut changes), 1, "one alert, not one per call");

    bob.workspace.age_huddles(Duration::from_millis(RING_QUIET_MS));
    call(&ada, &bob, &dm);
    assert_eq!(bob.media.rings_playing(), 1, "a minute later, a call rings again");
    assert_eq!(alerts(&mut changes), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_back_after_an_answered_one_rings() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let dm = dm_between(&ada, &bob).await;
    call(&ada, &bob, &dm);
    bob.workspace.join_huddle(&dm, false).unwrap();
    bob.workspace.leave_huddle().unwrap();
    hang_up(&ada, &bob);

    call(&ada, &bob, &dm);
    assert_eq!(bob.media.rings_playing(), 1);
}
