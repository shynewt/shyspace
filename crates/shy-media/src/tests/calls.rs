//! Two or three endpoints in one process exchange test tones over the real
//! media stack: node, policy, catalog, Opus encoding and decoding. Only the
//! devices are replaced.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use iroh::{Endpoint, PublicKey, address_lookup::memory::MemoryLookup, endpoint::presets, protocol::Router};
use iroh_live::Reach;
use moq_audio::decode;
use moq_mux::catalog::{self, CatalogFormat, Stream as _};
use shy_core::huddle::media::{DeviceChoice, HuddleMedia, Link, MediaCall, MediaEvent, MicState};
use tokio::sync::{mpsc, watch};

use super::tones::{Recorder, Tones, pitch};
use crate::{Media, WorkspaceMedia};

const A: f32 = 440.0;
const E: f32 = 660.0;
const G: f32 = 784.0;
/// Generous: the first connection on a loaded test machine can be slow.
const WITHIN: Duration = Duration::from_secs(20);
/// How long a test listens to be sure nothing arrives.
const QUIET_FOR: Duration = Duration::from_secs(3);

/// One workspace on a device: its own endpoint, sharing the device's audio.
struct Device {
    id: PublicKey,
    media: Arc<WorkspaceMedia<Tones>>,
    speaker: Recorder,
    router: Router,
}

impl Device {
    async fn new(lookup: &MemoryLookup, frequency: f32) -> Self {
        Self::with(lookup, Tones::new(frequency)).await
    }

    async fn with(lookup: &MemoryLookup, tones: Tones) -> Self {
        let speaker = tones.speaker();
        Self::in_workspace(lookup, &Arc::new(Media::new(tones)), speaker).await
    }

    /// Another workspace on the device whose audio is `media`.
    async fn in_workspace(lookup: &MemoryLookup, media: &Arc<Media<Tones>>, speaker: Recorder) -> Self {
        let endpoint = Endpoint::builder(presets::Minimal)
            .address_lookup(lookup.clone())
            .bind()
            .await
            .expect("bind a test endpoint");
        lookup.add_endpoint_info(endpoint.addr());
        let media = Arc::new(WorkspaceMedia::new(media.clone()));
        let router = media.attach(&endpoint, Router::builder(endpoint.clone())).spawn();
        Self {
            id: endpoint.id(),
            media,
            speaker,
            router,
        }
    }

    /// Starts a call at `path` with `peers` listed, as the core would.
    fn join(&self, path: &str, peers: &[(&Device, &str)]) -> Call {
        let (list, listed) = watch::channel(listing(peers));
        let (sender, events) = mpsc::channel(256);
        self.media
            .start(MediaCall {
                my_path: path.into(),
                peers: listed,
                events: sender,
                muted: false,
                devices: DeviceChoice::default(),
            })
            .expect("start a call");
        Call { list, events }
    }
}

struct Call {
    list: watch::Sender<BTreeMap<PublicKey, String>>,
    events: mpsc::Receiver<MediaEvent>,
}

impl Call {
    /// Waits for an event `wanted` picks; the ones before it are passed over.
    async fn event(&mut self, wanted: impl Fn(&MediaEvent) -> bool) {
        self.events_all(&[&wanted]).await;
    }

    /// Waits until each of `wanted` has picked an event, in any order.
    async fn events_all(&mut self, wanted: &[&dyn Fn(&MediaEvent) -> bool]) {
        let mut seen = vec![false; wanted.len()];
        tokio::time::timeout(WITHIN, async {
            while seen.contains(&false) {
                let event = self.events.recv().await.expect("the call reports events");
                for (index, picks) in wanted.iter().enumerate() {
                    seen[index] |= picks(&event);
                }
            }
        })
        .await
        .expect("the events arrive in time");
    }
}

fn listing(peers: &[(&Device, &str)]) -> BTreeMap<PublicKey, String> {
    peers
        .iter()
        .map(|(device, path)| (device.id, (*path).to_owned()))
        .collect()
}

/// Whether the latest quarter second of some stream is a clear tone at `frequency`.
fn is_hearing(speaker: &Recorder, frequency: f32) -> bool {
    let recent = 12_000;
    speaker.streams().iter().any(|stream| {
        if stream.len() < recent {
            return false;
        }
        let latest = &stream[stream.len() - recent..];
        let loud = crate::listen::rms(latest) > 0.1;
        loud && (pitch(latest) - frequency).abs() < frequency * 0.05
    })
}

async fn hears(speaker: &Recorder, frequency: f32) -> bool {
    let deadline = Instant::now() + WITHIN;
    while Instant::now() < deadline {
        if is_hearing(speaker, frequency) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// Whether nothing more reaches `speaker` once it has had a moment to stop.
async fn goes_quiet(speaker: &Recorder) -> bool {
    tokio::time::sleep(Duration::from_secs(1)).await;
    let before = speaker.total();
    tokio::time::sleep(QUIET_FOR).await;
    speaker.total() == before
}

#[tokio::test(flavor = "multi_thread")]
async fn listed_peers_hear_each_others_voices() {
    let lookup = MemoryLookup::new();
    let alice = Device::new(&lookup, A).await;
    let bob = Device::new(&lookup, E).await;
    let mut alices_call = alice.join("h-alice", &[(&bob, "h-bob")]);
    let mut bobs_call = bob.join("h-bob", &[(&alice, "h-alice")]);

    assert!(hears(&bob.speaker, A).await, "Bob hears Alice's tone");
    assert!(hears(&alice.speaker, E).await, "Alice hears Bob's tone");

    let alice_id = alice.id;
    bobs_call
        .events_all(&[
            &|event| matches!(event, MediaEvent::Speaking { peer, speaking: true } if *peer == alice_id),
            &|event| matches!(event, MediaEvent::Link { peer, link: Link::Direct } if *peer == alice_id),
        ])
        .await;
    alices_call
        .events_all(&[&|event| *event == MediaEvent::Mic(MicState::Live), &|event| {
            *event == MediaEvent::MeSpeaking(true)
        }])
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn three_peers_hear_the_other_two_mixed() {
    let lookup = MemoryLookup::new();
    let alice = Device::new(&lookup, A).await;
    let bob = Device::new(&lookup, E).await;
    let carol = Device::new(&lookup, G).await;
    let _alices_call = alice.join("h-alice", &[(&bob, "h-bob"), (&carol, "h-carol")]);
    let _bobs_call = bob.join("h-bob", &[(&alice, "h-alice"), (&carol, "h-carol")]);
    let _carols_call = carol.join("h-carol", &[(&alice, "h-alice"), (&bob, "h-bob")]);

    assert!(hears(&carol.speaker, A).await, "Carol hears Alice");
    assert!(hears(&carol.speaker, E).await, "Carol hears Bob");
    assert!(hears(&alice.speaker, G).await, "Alice hears Carol");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_outsider_who_knows_the_path_hears_nothing() {
    let lookup = MemoryLookup::new();
    let alice = Device::new(&lookup, A).await;
    let bob = Device::new(&lookup, E).await;
    let eve = Device::new(&lookup, G).await;
    let _alices_call = alice.join("h-alice", &[(&bob, "h-bob")]);
    let _bobs_call = bob.join("h-bob", &[(&alice, "h-alice")]);
    let _eves_call = eve.join("h-eve", &[(&alice, "h-alice")]);
    assert!(hears(&bob.speaker, A).await, "the call itself works");

    let (eves_node, _) = eve.media.attached().expect("attached");
    let subscribed =
        tokio::time::timeout(WITHIN, eves_node.subscribe("h-alice", Reach::Direct(alice.id))).await;
    match subscribed {
        Ok(Ok(_)) => panic!("an outsider subscribed to the call"),
        Ok(Err(_refused)) => {}
        Err(_) => panic!("the outsider's attempt hung instead of being refused"),
    }
    assert_eq!(eve.speaker.total(), 0, "an outsider heard the call");
}

/// A device in two workspaces has an endpoint in each. A call runs on its
/// own workspace's, though the other opened last, and the other stopping
/// its huddle audio or closing its network leaves the call alone.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_runs_on_its_own_workspaces_network() {
    let lookup = MemoryLookup::new();
    let tones = Tones::new(A);
    let speaker = tones.speaker();
    let alices_audio = Arc::new(Media::new(tones));
    let alice = Device::in_workspace(&lookup, &alices_audio, speaker.clone()).await;
    let alice_elsewhere = Device::in_workspace(&lookup, &alices_audio, speaker).await;
    let bob = Device::new(&lookup, E).await;
    let _alices_call = alice.join("h-alice", &[(&bob, "h-bob")]);
    let _bobs_call = bob.join("h-bob", &[(&alice, "h-alice")]);
    assert!(hears(&bob.speaker, A).await, "Bob hears Alice");
    assert!(hears(&alice.speaker, E).await, "Alice hears Bob");

    alice_elsewhere.media.stop();
    alice_elsewhere.media.set_muted(true).expect("mute elsewhere");
    alice_elsewhere
        .router
        .shutdown()
        .await
        .expect("close the other network");

    assert!(!goes_quiet(&bob.speaker).await, "Bob stopped hearing Alice");
    assert!(is_hearing(&alice.speaker, E), "Alice stopped hearing Bob");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peer_taken_off_the_list_stops_hearing_at_once() {
    let lookup = MemoryLookup::new();
    let alice = Device::new(&lookup, A).await;
    let bob = Device::new(&lookup, E).await;
    let alices_call = alice.join("h-alice", &[(&bob, "h-bob")]);
    // Bob keeps Alice listed and keeps trying, as a removed member's device might.
    let _bobs_call = bob.join("h-bob", &[(&alice, "h-alice")]);
    assert!(hears(&bob.speaker, A).await, "Bob hears Alice at first");

    alices_call.list.send_replace(BTreeMap::new());

    assert!(goes_quiet(&bob.speaker).await, "Bob still hears Alice");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_that_stops_cuts_everyone_off() {
    let lookup = MemoryLookup::new();
    let alice = Device::new(&lookup, A).await;
    let bob = Device::new(&lookup, E).await;
    let _alices_call = alice.join("h-alice", &[(&bob, "h-bob")]);
    let _bobs_call = bob.join("h-bob", &[(&alice, "h-alice")]);
    assert!(hears(&bob.speaker, A).await, "Bob hears Alice at first");

    alice.media.stop();

    assert!(goes_quiet(&bob.speaker).await, "Bob still hears Alice");
}

#[tokio::test(flavor = "multi_thread")]
async fn muting_closes_the_microphone_and_unmuting_opens_it_again() {
    let lookup = MemoryLookup::new();
    let alice = Device::new(&lookup, A).await;
    let bob = Device::new(&lookup, E).await;
    let mut alices_call = alice.join("h-alice", &[(&bob, "h-bob")]);
    let _bobs_call = bob.join("h-bob", &[(&alice, "h-alice")]);
    assert!(hears(&bob.speaker, A).await, "Bob hears Alice at first");

    alice.media.set_muted(true).expect("mute");
    alices_call
        .event(|event| *event == MediaEvent::Mic(MicState::Off))
        .await;
    assert!(
        goes_quiet(&bob.speaker).await,
        "Bob hears Alice while she's muted"
    );

    alice.media.set_muted(false).expect("unmute");
    alices_call
        .event(|event| *event == MediaEvent::Mic(MicState::Opening))
        .await;
    alices_call
        .event(|event| *event == MediaEvent::Mic(MicState::Live))
        .await;
    assert!(hears(&bob.speaker, A).await, "Bob hears Alice again");
}

#[tokio::test(flavor = "multi_thread")]
async fn someone_let_in_late_cant_replay_what_was_said_before() {
    let lookup = MemoryLookup::new();
    let alice = Device::new(&lookup, A).await;
    let bob = Device::new(&lookup, E).await;
    let alices_call = alice.join("h-alice", &[]);
    let _bobs_call = bob.join("h-bob", &[(&alice, "h-alice")]);
    // Alice speaks alone for a while before Bob may hear her.
    tokio::time::sleep(Duration::from_secs(3)).await;
    alices_call.list.send_replace(listing(&[(&bob, "h-bob")]));

    // A modified client asks for the oldest audio there is.
    let (bobs_node, _) = bob.media.attached().expect("attached");
    let deadline = Instant::now() + WITHIN;
    let subscription = loop {
        let attempt = async { bobs_node.connect(alice.id).await?.subscribe("h-alice").await };
        if let Ok(subscription) = attempt.await {
            break subscription;
        }
        assert!(Instant::now() < deadline, "Bob can't reach Alice once let in");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let broadcast = subscription.broadcast();
    let mut catalog = catalog::Consumer::<()>::new(&broadcast, CatalogFormat::Hang)
        .await
        .expect("read Alice's catalog");
    let snapshot = catalog.next().await.expect("a catalog").expect("a catalog");
    let (name, config) = snapshot
        .audio
        .renditions
        .first_key_value()
        .expect("an audio track");
    let mut options = decode::Options::new();
    options.output.format = moq_audio::Format::F32;
    options.start = decode::Start::Oldest;
    // and to keep audio however old it is. (Asking to wait longer than the
    // track keeps is refused by the library, which a modified client
    // wouldn't use, so this checks what the publisher still holds.)
    options.delay.max = None;
    let mut decoder = decode::Consumer::new(&broadcast, config, name, options)
        .await
        .expect("open Alice's audio");
    let channels = decoder.layout().channels() as usize;
    let rate = decoder.sample_rate() as f32;

    let listening = Duration::from_secs(1);
    let started = Instant::now();
    let mut heard = Duration::ZERO;
    while let Ok(Ok(Some(read))) = tokio::time::timeout_at((started + listening).into(), decoder.read()).await
    {
        if let decode::Read::Frame(frame) = read {
            heard += Duration::from_secs_f32((frame.data.len() / 4 / channels) as f32 / rate);
        }
    }
    assert!(
        heard < listening + Duration::from_millis(500),
        "Bob got {heard:?} of audio in {listening:?}: some of it is from before he was let in"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peer_copying_anothers_path_is_played_as_no_one() {
    let lookup = MemoryLookup::new();
    let alice = Device::new(&lookup, A).await;
    let bob = Device::new(&lookup, E).await;
    let carol = Device::new(&lookup, G).await;
    // Carol copies Alice's session from her beacon, so the lists name both
    // at Alice's path.
    let _carols_call = carol.join("h-alice", &[(&bob, "h-bob"), (&alice, "h-alice-real")]);
    let _bobs_call = bob.join("h-bob", &[(&alice, "h-alice"), (&carol, "h-alice")]);
    let _alices_call = alice.join("h-alice", &[(&bob, "h-bob"), (&carol, "h-alice")]);

    assert!(
        goes_quiet(&bob.speaker).await,
        "Bob played someone at a copied path"
    );
    assert!(!is_hearing(&bob.speaker, G), "Bob played Carol as Alice");
    assert!(goes_quiet(&alice.speaker).await, "Alice played her own path back");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_broken_microphone_stays_lost_while_it_is_retried() {
    let lookup = MemoryLookup::new();
    let alice = Device::with(&lookup, Tones::with_a_broken_microphone(A)).await;
    let mut call = alice.join("h-alice", &[]);
    call.event(|event| *event == MediaEvent::Mic(MicState::Lost))
        .await;

    // Two retries go by.
    tokio::time::sleep(Duration::from_secs(7)).await;
    let mut later = Vec::new();
    while let Ok(event) = call.events.try_recv() {
        later.push(event);
    }
    assert!(
        !later.iter().any(|event| matches!(event, MediaEvent::Mic(_))),
        "the microphone's state flickered while retrying: {later:?}"
    );
}
