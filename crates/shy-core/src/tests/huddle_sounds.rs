//! The sounds a huddle plays as people come and go: one per change, none
//! for the people already there when you join, and none once turned off.

use std::time::Duration;

use super::{
    huddles::{carry, three_members},
    support::*,
};
use crate::{
    huddle::{book::EXPIRY_MS, media::Cue},
    views::Change,
    workspace::Via,
};

#[tokio::test(flavor = "multi_thread")]
async fn joining_and_leaving_play_their_sounds() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    assert_eq!(ada.media.cues(), [Cue::Joined]);
    ada.workspace.leave_huddle().unwrap();
    assert_eq!(ada.media.cues(), [Cue::Joined, Cue::Left]);
}

#[tokio::test(flavor = "multi_thread")]
async fn others_coming_and_going_play_their_sounds() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    bob.workspace.join_huddle(&general, false).unwrap();
    carry(&bob.workspace, &[&ada.workspace], Via::Gossip);
    bob.workspace.leave_huddle().unwrap();
    carry(&bob.workspace, &[&ada.workspace], Via::Gossip);
    assert_eq!(ada.media.cues(), [Cue::Joined, Cue::PeerJoined, Cue::PeerLeft]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_people_already_there_when_you_join_make_no_sound() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    bob.workspace.join_huddle(&general, false).unwrap();
    carry(&ada.workspace, &[&carol.workspace], Via::Gossip);
    carry(&bob.workspace, &[&carol.workspace], Via::Gossip);
    carol.workspace.join_huddle(&general, false).unwrap();
    assert_eq!(carol.media.peers().len(), 2);
    assert_eq!(carol.media.cues(), [Cue::Joined]);
}

#[tokio::test(flavor = "multi_thread")]
async fn several_people_leaving_at_once_make_one_sound() {
    let (_root, [ada, bob, carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    ada.workspace.join_huddle(&general, false).unwrap();
    for other in [&bob, &carol] {
        other.workspace.join_huddle(&general, false).unwrap();
        carry(&other.workspace, &[&ada.workspace], Via::Gossip);
    }
    assert_eq!(ada.media.peers().len(), 2);

    // Both stop refreshing, so they expire together.
    ada.workspace.age_huddles(Duration::from_millis(EXPIRY_MS + 1));
    ada.workspace.expire_huddle_participants().unwrap();
    assert!(ada.media.peers().is_empty());
    assert_eq!(
        ada.media.cues(),
        [Cue::Joined, Cue::PeerJoined, Cue::PeerJoined, Cue::PeerLeft]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_huddles_plays_only_the_join() {
    let (_root, [ada, _bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    let design = ada.workspace.create_channel("design").unwrap();
    ada.workspace.join_huddle(&general, false).unwrap();
    ada.workspace.join_huddle(&design, false).unwrap();
    assert_eq!(ada.media.cues(), [Cue::Joined, Cue::Joined]);
}

#[tokio::test(flavor = "multi_thread")]
async fn sounds_turned_off_stay_silent() {
    let (_root, [ada, bob, _carol]) = three_members().await;
    let general = channel_id(&ada.workspace, "general");
    assert!(ada.workspace.huddle_sounds().unwrap(), "sounds start on");
    let mut changes = ada.workspace.subscribe();
    ada.workspace.set_huddle_sounds(false).unwrap();
    assert!(!ada.workspace.huddle_sounds().unwrap());
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(changes.recv().await, Ok(Change::HuddleSounds)) {}
    })
    .await
    .expect("no huddle_sounds change");

    ada.workspace.join_huddle(&general, false).unwrap();
    bob.workspace.join_huddle(&general, false).unwrap();
    carry(&bob.workspace, &[&ada.workspace], Via::Gossip);
    ada.workspace.leave_huddle().unwrap();
    assert!(ada.media.cues().is_empty());
}
