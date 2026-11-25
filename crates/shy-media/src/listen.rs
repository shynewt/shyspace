//! Hearing one peer: subscribes to their path, plays their voice into the
//! shared speaker, and reports the link and whether they're speaking. Keeps
//! trying until the call drops the peer, since either side may list the
//! other a moment before the other side lists it back.

use std::time::{Duration, Instant};

use iroh::PublicKey;
use iroh_live::{NetworkPathKind, Node, Subscription};
use moq_audio::decode;
use moq_mux::catalog::{self, CatalogFormat, Stream as _};
use shy_core::huddle::media::{Link, MediaEvent};
use tokio::sync::mpsc;
use tracing::debug;

use crate::{
    backend::{Sink, Speaker},
    mic::until,
    speaking::SpeakingDetector,
};

/// How long one attempt to reach the peer may take.
const SUBSCRIBE_WAIT: Duration = Duration::from_secs(10);
/// The pause between attempts.
const RETRY_AFTER: Duration = Duration::from_secs(1);
/// How often the link's route is looked at.
const LINK_EVERY: Duration = Duration::from_secs(1);
/// After audio that won't decode, the pause before decoding the track
/// afresh: doubled while it keeps failing, up to the most, and back to the
/// least only after a while of good audio, so a peer mixing garbage into
/// good audio costs little. A catalog republish also restarts decoding
/// (moq-mux sends one whenever a track's jitter or bitrate estimate moves),
/// but nothing makes one follow a bad packet.
const REDECODE_AFTER: Duration = Duration::from_millis(100);
const REDECODE_AFTER_MOST: Duration = Duration::from_secs(2);
const GOOD_FOR: Duration = Duration::from_secs(1);
/// How far a peer's decoded audio may run ahead of the clock (a burst after
/// a stall, the jitter buffer) before decoding stops, and how much unused
/// time it may save up: a peer sending faster than real time would
/// otherwise keep this device decoding all of it.
const AHEAD_AT_MOST: Duration = Duration::from_secs(1);
/// A voice needs no more; more channels only cost decoding.
const MAX_CHANNELS: u32 = 2;
/// The most audio a peer may have decoded and thrown away at the start of
/// a track (Opus pre-skip, in 48 kHz samples, and the codec's warmup):
/// what our own encoder asks for. Decoding it is never counted as played.
const MAX_PRE_SKIP: u16 = 3_840;
const MAX_WARMUP: Duration = moq_mux::codec::opus::WARMUP;

/// Plays `peer`'s voice from `path` until dropped.
pub(crate) async fn listen<S: Speaker>(
    node: Node,
    peer: PublicKey,
    path: String,
    speaker: S,
    events: mpsc::Sender<MediaEvent>,
) {
    let mut link = LinkReport::new(peer, events.clone());
    link.set(Link::Connecting);
    let mut voice = PeerVoice::new(peer, events.clone());
    // One for the whole call, so restarting their audio saves them nothing.
    let mut clock = AudioClock::new(Instant::now());
    loop {
        match tokio::time::timeout(SUBSCRIBE_WAIT, subscribe(&node, peer, &path)).await {
            Ok(Ok(subscription)) => {
                tokio::select! {
                    () = play(&subscription, &speaker, &mut voice, &mut clock) => {}
                    () = subscription.closed() => {}
                    () = follow_link(&subscription, &mut link) => {}
                }
                voice.stopped();
                link.set(Link::Reconnecting);
            }
            Ok(Err(error)) => {
                debug!(peer = %peer.fmt_short(), error = %error, "can't reach a huddle peer yet")
            }
            Err(_) => debug!(peer = %peer.fmt_short(), "reaching a huddle peer timed out"),
        }
        tokio::time::sleep(RETRY_AFTER).await;
    }
}

/// Reads `path` from `peer`'s own session only, so another device that
/// announces the same path can't be played as them.
async fn subscribe(node: &Node, peer: PublicKey, path: &str) -> Result<Subscription, iroh_live::node::Error> {
    node.connect(peer).await?.subscribe(path).await
}

/// Reports whether the route to the peer is direct or relayed.
async fn follow_link(subscription: &Subscription, link: &mut LinkReport) {
    let mut every = tokio::time::interval(LINK_EVERY);
    loop {
        every.tick().await;
        let selected = subscription
            .session()
            .and_then(|session| session.network_paths().into_iter().next());
        if let Some(path) = selected {
            link.set(match path.kind() {
                NetworkPathKind::Relay => Link::Relayed,
                _ => Link::Direct,
            });
        }
    }
}

/// Follows the peer's catalog and plays its first audio track, until the
/// broadcast ends.
async fn play<S: Speaker>(
    subscription: &Subscription,
    speaker: &S,
    voice: &mut PeerVoice,
    clock: &mut AudioClock,
) {
    let broadcast = subscription.broadcast();
    let mut catalog = match catalog::Consumer::<()>::new(&broadcast, CatalogFormat::Hang).await {
        Ok(catalog) => catalog,
        Err(error) => {
            debug!(error = %error, "a huddle peer's catalog can't be read");
            return;
        }
    };
    let decoding = || {
        let mut options = decode::Options::new();
        options.output.format = moq_audio::Format::F32;
        options.start = decode::Start::Latest;
        options
    };
    let mut playing: Option<Playing<S::Sink>> = None;
    // The track being played (its name and config), to decode again after a failure.
    let mut current = None;
    let mut restarts = Restarts::default();
    loop {
        tokio::select! {
            snapshot = catalog.next() => {
                let Ok(Some(snapshot)) = snapshot else { return };
                current = None;
                let Some((name, config)) = snapshot.audio.renditions.first_key_value() else {
                    // Muted: their track is gone until they unmute.
                    playing = None;
                    voice.stopped();
                    continue;
                };
                if playing.as_ref().is_some_and(|playing| playing.name == *name) {
                    continue;
                }
                // Huddles speak Opus only, which keeps what a peer can make
                // this device decode small.
                if config.codec.to_string() != "opus" {
                    playing = None;
                    voice.stopped();
                    voice.incompatible();
                    continue;
                }
                let mut config = config.clone();
                config.warmup = config.warmup.map(|warmup| warmup.min(MAX_WARMUP));
                match config.description.as_deref().map(bounded_opus_head) {
                    Some(Ok(Some(bounded))) => config.description = Some(bounded.into()),
                    Some(Err(error)) => {
                        debug!(error = %error, "a huddle peer's Opus header is refused");
                        playing = None;
                        voice.stopped();
                        voice.incompatible();
                        continue;
                    }
                    None | Some(Ok(None)) => {}
                }
                current = Some((name.clone(), config));
                if !restarts.may_start_now(tokio::time::Instant::now()) {
                    if playing.take().is_some() {
                        voice.stopped();
                    }
                    continue;
                }
                let Some((name, config)) = &current else { continue };
                let decoder = decode::Consumer::new(&broadcast, config, name, decoding()).await;
                playing = start_playing(decoder, name, speaker, voice);
            }
            () = until(restarts.pending()), if playing.is_none() => {
                if restarts.restart(tokio::time::Instant::now()) == Forgive::Yes {
                    clock.forgive(Instant::now());
                }
                let Some((name, config)) = &current else { continue };
                let decoder = decode::Consumer::new(&broadcast, config, name, decoding()).await;
                playing = start_playing(decoder, name, speaker, voice);
            }
            read = async { playing.as_mut().expect("guarded").decoder.read().await }, if playing.is_some() => {
                let Some(now_playing) = playing.as_mut() else { continue };
                match read {
                    Ok(Some(decode::Read::Frame(frame))) => {
                        restarts.played(tokio::time::Instant::now());
                        match now_playing.play(&frame.data, voice, clock) {
                            Ok(Pace::Ahead) => {
                                debug!("a huddle peer sends faster than real time; pausing their audio");
                                playing = None;
                                voice.stopped();
                                restarts.pause(Pause::RanAhead, tokio::time::Instant::now());
                            }
                            Ok(Pace::InTime) => {}
                            Err(error) => debug!(error = %error, "a huddle peer's audio can't be played"),
                        }
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        playing = None;
                        voice.stopped();
                    }
                    Err(error) => {
                        debug!(error = %error, "a huddle peer's audio can't be decoded; starting it again");
                        playing = None;
                        voice.stopped();
                        restarts.pause(Pause::Failed, tokio::time::Instant::now());
                    }
                }
            }
        }
    }
}

type Moment = tokio::time::Instant;

/// Why decoding paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pause {
    /// The peer's audio ran ahead of the clock.
    RanAhead,
    /// Their audio couldn't be decoded.
    Failed,
}

/// Whether a restart forgives part of the peer's lead on the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Forgive {
    Yes,
    No,
}

/// When a peer's decoder may start again. Each start decodes and throws
/// away the track's first moments, uncounted, so starts are spaced: after a
/// pause (doubling while it keeps failing, back to the shortest only after
/// a while of good audio from the restart), and a new catalog waits out the
/// pause, and the shortest one since the last start.
#[derive(Debug)]
struct Restarts {
    at: Option<Moment>,
    next_pause: Duration,
    /// When the last pause began, or its restart once it ended.
    last_failure: Option<Moment>,
    last_start: Option<Moment>,
    /// The pause under way followed running ahead, the only kind that
    /// forgives: those double, while a catalog's wait is always the shortest.
    is_pace_pause: bool,
}

impl Default for Restarts {
    fn default() -> Self {
        Self {
            at: None,
            next_pause: REDECODE_AFTER,
            last_failure: None,
            last_start: None,
            is_pace_pause: false,
        }
    }
}

impl Restarts {
    fn pending(&self) -> Option<Moment> {
        self.at
    }

    fn pause(&mut self, why: Pause, now: Moment) {
        self.is_pace_pause = why == Pause::RanAhead;
        self.last_failure = Some(now);
        self.at = Some(now + self.next_pause);
        self.next_pause = (self.next_pause * 2).min(REDECODE_AFTER_MOST);
    }

    /// Whether a new catalog may start a decoder now; if not, it starts
    /// when the wait ends.
    fn may_start_now(&mut self, now: Moment) -> bool {
        let earliest = self.last_start.map(|started| started + REDECODE_AFTER);
        if self.at.is_some() || earliest.is_some_and(|at| at > now) {
            self.at = self.at.or(earliest);
            return false;
        }
        self.last_start = Some(now);
        true
    }

    /// The wait ended and the decoder starts again.
    fn restart(&mut self, now: Moment) -> Forgive {
        self.at = None;
        self.last_start = Some(now);
        // Good audio counts from here, so the pause itself doesn't reset
        // the backoff.
        if self.last_failure.is_some() {
            self.last_failure = Some(now);
        }
        match std::mem::take(&mut self.is_pace_pause) {
            true => Forgive::Yes,
            false => Forgive::No,
        }
    }

    fn played(&mut self, now: Moment) {
        if self
            .last_failure
            .is_some_and(|failed| now.duration_since(failed) >= GOOD_FOR)
        {
            self.next_pause = REDECODE_AFTER;
            self.last_failure = None;
        }
    }
}

fn start_playing<S: Speaker>(
    decoder: Result<decode::Consumer, moq_audio::Error>,
    name: &str,
    speaker: &S,
    voice: &PeerVoice,
) -> Option<Playing<S::Sink>> {
    let decoder = match decoder {
        Ok(decoder) => decoder,
        Err(moq_audio::Error::UnsupportedConfig(_) | moq_audio::Error::Unsupported(_)) => {
            voice.incompatible();
            return None;
        }
        Err(error) => {
            debug!(error = %error, "a huddle peer's audio can't be opened");
            return None;
        }
    };
    if decoder.layout().channels() > MAX_CHANNELS {
        voice.incompatible();
        return None;
    }
    let sink = match speaker.sink(decoder.sample_rate(), decoder.layout()) {
        Ok(sink) => sink,
        Err(error) => {
            debug!(error = %error, "the speaker can't take a huddle peer's audio");
            return None;
        }
    };
    Some(Playing {
        name: name.to_owned(),
        channels: decoder.layout().channels(),
        sample_rate: decoder.sample_rate(),
        decoder,
        sink,
    })
}

/// The peer's Opus header within what a voice needs: no gain above 0 dB,
/// so nobody sounds louder than they sent, and a pre-skip of at most
/// `MAX_PRE_SKIP`. `None` keeps it as it is; a header that doesn't parse is
/// left for the decoder to refuse. Refuses a channel mapping, which can
/// pack many streams to decode into each frame.
fn bounded_opus_head(mut description: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
    let Ok(mut head) = moq_mux::codec::opus::Config::parse(&mut description) else {
        return Ok(None);
    };
    // Without one, Opus allows at most two channels.
    anyhow::ensure!(head.mapping.is_none(), "an Opus channel mapping");
    let bounded_gain = head.output_gain.min(0);
    let bounded_pre_skip = head.pre_skip.min(MAX_PRE_SKIP);
    if (bounded_gain, bounded_pre_skip) == (head.output_gain, head.pre_skip) {
        return Ok(None);
    }
    head.output_gain = bounded_gain;
    head.pre_skip = bounded_pre_skip;
    Ok(Some(head.encode()?.to_vec()))
}

/// Whether a peer's audio keeps to the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pace {
    InTime,
    Ahead,
}

/// How much of a peer's audio was decoded, against how long they've been
/// heard. Time they used no audio is saved up only to `AHEAD_AT_MOST`, so
/// a quiet spell can't buy a burst.
struct AudioClock {
    started: Instant,
    decoded: Duration,
}

impl AudioClock {
    fn new(started: Instant) -> Self {
        Self {
            started,
            decoded: Duration::ZERO,
        }
    }

    fn decoded(&mut self, audio: Duration, now: Instant) -> Pace {
        let saved_up = now.duration_since(self.started).saturating_sub(self.decoded);
        if saved_up > AHEAD_AT_MOST {
            self.started += saved_up - AHEAD_AT_MOST;
        }
        self.decoded += audio;
        match self.decoded > now.duration_since(self.started) + AHEAD_AT_MOST {
            true => Pace::Ahead,
            false => Pace::InTime,
        }
    }

    /// After a pause, leaves the peer at most half the allowance ahead, so
    /// an honest device whose clock runs a little fast isn't left at the
    /// edge, tripping again soon after; a peer racing ahead gains only that
    /// half per pause.
    fn forgive(&mut self, now: Instant) {
        let allowed = now.duration_since(self.started) + AHEAD_AT_MOST / 2;
        self.decoded = self.decoded.min(allowed);
    }
}

struct Playing<K: Sink> {
    name: String,
    channels: u32,
    sample_rate: u32,
    decoder: decode::Consumer,
    sink: K,
}

impl<K: Sink> Playing<K> {
    fn play(&mut self, data: &[u8], voice: &mut PeerVoice, clock: &mut AudioClock) -> anyhow::Result<Pace> {
        let samples = moq_audio::Format::F32.as_interleaved_f32(data, self.channels)?;
        let given = samples.len() / self.channels.max(1) as usize;
        let audio = Duration::from_secs_f64(given as f64 / f64::from(self.sample_rate.max(1)));
        if clock.decoded(audio, Instant::now()) == Pace::Ahead {
            return Ok(Pace::Ahead);
        }
        voice.heard(rms(&samples));
        // Live audio that can't keep up is dropped rather than played late.
        let taken = self.sink.write(&samples)?;
        if taken < given {
            debug!(dropped = given - taken, "the speaker dropped late audio");
        }
        Ok(Pace::InTime)
    }
}

pub(crate) fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|sample| sample * sample).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Reports a peer's speaking, and when their audio can't be read.
struct PeerVoice {
    peer: PublicKey,
    events: mpsc::Sender<MediaEvent>,
    detector: SpeakingDetector,
}

impl PeerVoice {
    fn new(peer: PublicKey, events: mpsc::Sender<MediaEvent>) -> Self {
        Self {
            peer,
            events,
            detector: SpeakingDetector::default(),
        }
    }

    fn heard(&mut self, level: f32) {
        if let Some(speaking) = self.detector.observe(level, Instant::now()) {
            self.report_speaking(speaking);
        }
    }

    /// Their audio stopped: they aren't speaking any more.
    fn stopped(&mut self) {
        if self.detector.stop() {
            self.report_speaking(false);
        }
    }

    fn report_speaking(&self, speaking: bool) {
        let _ = self.events.try_send(MediaEvent::Speaking {
            peer: self.peer,
            speaking,
        });
    }

    fn incompatible(&self) {
        let _ = self.events.try_send(MediaEvent::Incompatible { peer: self.peer });
    }
}

/// Sends a peer's link state when it changes.
struct LinkReport {
    peer: PublicKey,
    events: mpsc::Sender<MediaEvent>,
    last: Option<Link>,
}

impl LinkReport {
    fn new(peer: PublicKey, events: mpsc::Sender<MediaEvent>) -> Self {
        Self {
            peer,
            events,
            last: None,
        }
    }

    fn set(&mut self, link: Link) {
        if self.last == Some(link) {
            return;
        }
        // Until the first connection, a failed attempt is still connecting.
        if link == Link::Reconnecting && matches!(self.last, None | Some(Link::Connecting)) {
            return;
        }
        self.last = Some(link);
        let _ = self.events.try_send(MediaEvent::Link {
            peer: self.peer,
            link,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(change: impl FnOnce(&mut moq_mux::codec::opus::Config)) -> Vec<u8> {
        let mut head = moq_mux::codec::opus::Config::new(48_000, 1).with_pre_skip(312);
        change(&mut head);
        head.encode().unwrap().to_vec()
    }

    fn parsed(mut description: &[u8]) -> moq_mux::codec::opus::Config {
        moq_mux::codec::opus::Config::parse(&mut description).unwrap()
    }

    #[test]
    fn a_peer_asking_for_more_gain_plays_at_full_volume_at_most() {
        let bounded = bounded_opus_head(&head(|head| head.output_gain = i16::MAX))
            .unwrap()
            .unwrap();
        assert_eq!(parsed(&bounded).output_gain, 0);
        assert_eq!(
            bounded_opus_head(&head(|head| head.output_gain = -1541)).unwrap(),
            None,
            "quieter stays"
        );
        assert_eq!(
            bounded_opus_head(&head(|_| {})).unwrap(),
            None,
            "our own header stays"
        );
        assert_eq!(
            bounded_opus_head(b"junk").unwrap(),
            None,
            "left for the decoder to refuse"
        );
    }

    #[test]
    fn a_peer_cant_make_this_device_decode_more_than_it_plays() {
        let bounded = bounded_opus_head(&head(|head| head.pre_skip = u16::MAX))
            .unwrap()
            .unwrap();
        assert_eq!(parsed(&bounded).pre_skip, MAX_PRE_SKIP);
        let mapping = moq_mux::codec::opus::Mapping::new(moq_mux::codec::opus::mapping::Config {
            family: 1,
            streams: 127,
            coupled: 127,
            table: &[0, 1],
        })
        .unwrap();
        let packed = head(|head| {
            head.channel_count = 2;
            head.mapping = Some(mapping);
        });
        assert!(
            bounded_opus_head(&packed).is_err(),
            "many streams behind two channels"
        );
    }

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn audio_running_ahead_of_the_clock_is_caught() {
        let started = Instant::now();
        let mut clock = AudioClock::new(started);
        assert_eq!(
            clock.decoded(SECOND, started),
            Pace::InTime,
            "a burst of a second"
        );
        assert_eq!(clock.decoded(Duration::from_millis(20), started), Pace::Ahead);

        let mut clock = AudioClock::new(started);
        for second in 1..=10 {
            let now = started + SECOND * second;
            assert_eq!(
                clock.decoded(SECOND, now),
                Pace::InTime,
                "real time, second {second}"
            );
        }
    }

    #[test]
    fn a_pause_leaves_a_fast_clock_room_to_play_on() {
        let started = Instant::now();
        let mut clock = AudioClock::new(started);
        let now = started + SECOND * 60;
        assert_eq!(clock.decoded(SECOND * 62, now), Pace::Ahead);

        clock.forgive(now);

        assert_eq!(clock.decoded(SECOND * 4 / 10, now), Pace::InTime);
        assert_eq!(
            clock.decoded(SECOND * 2 / 10, now),
            Pace::Ahead,
            "only half the allowance"
        );
    }

    #[test]
    fn switching_tracks_quickly_never_earns_forgiveness_or_quick_starts() {
        let mut restarts = Restarts::default();
        let mut now = Moment::now();
        assert!(restarts.may_start_now(now));
        for _ in 0..20 {
            now += Duration::from_millis(50);
            assert!(!restarts.may_start_now(now), "within the shortest pause");
            let at = restarts.pending().unwrap();
            assert!(at >= now, "a start waits for its moment");
            now = at;
            assert_eq!(restarts.restart(now), Forgive::No);
        }
    }

    #[test]
    fn running_ahead_pauses_longer_each_time_and_forgives() {
        let mut restarts = Restarts::default();
        let mut now = Moment::now();
        assert!(restarts.may_start_now(now));
        let mut pauses = Vec::new();
        for _ in 0..6 {
            restarts.pause(Pause::RanAhead, now);
            let at = restarts.pending().unwrap();
            pauses.push(at - now);
            now = at;
            assert_eq!(restarts.restart(now), Forgive::Yes);
            now += Duration::from_millis(50);
            restarts.played(now);
        }
        let millis: Vec<u128> = pauses.iter().map(Duration::as_millis).collect();
        assert_eq!(millis, [100, 200, 400, 800, 1600, 2000]);

        now += GOOD_FOR;
        restarts.played(now);
        restarts.pause(Pause::Failed, now);
        assert_eq!(
            restarts.pending().unwrap() - now,
            REDECODE_AFTER,
            "a second of good audio resets it"
        );
        assert_eq!(restarts.restart(now + REDECODE_AFTER), Forgive::No);
    }

    #[test]
    fn a_quiet_spell_doesnt_buy_a_burst() {
        let started = Instant::now();
        let mut clock = AudioClock::new(started);
        let after_a_minute = started + SECOND * 60;

        assert_eq!(clock.decoded(SECOND * 2, after_a_minute), Pace::InTime);
        assert_eq!(clock.decoded(SECOND / 10, after_a_minute), Pace::Ahead);
    }
}
