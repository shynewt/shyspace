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
/// afresh: doubled while it keeps failing, up to the most, so a peer
/// sending garbage costs little.
const REDECODE_AFTER: Duration = Duration::from_millis(100);
const REDECODE_AFTER_MOST: Duration = Duration::from_secs(2);

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
    loop {
        match tokio::time::timeout(SUBSCRIBE_WAIT, subscribe(&node, peer, &path)).await {
            Ok(Ok(subscription)) => {
                tokio::select! {
                    () = play(&subscription, &speaker, &mut voice) => {}
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
async fn play<S: Speaker>(subscription: &Subscription, speaker: &S, voice: &mut PeerVoice) {
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
    let mut redecode_at: Option<tokio::time::Instant> = None;
    let mut redecode_after = REDECODE_AFTER;
    loop {
        tokio::select! {
            snapshot = catalog.next() => {
                let Ok(Some(snapshot)) = snapshot else { return };
                redecode_at = None;
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
                let decoder = decode::Consumer::new(&broadcast, config, name, decoding()).await;
                playing = start_playing(decoder, name, speaker, voice);
                current = Some((name.clone(), config.clone()));
            }
            () = until(redecode_at), if playing.is_none() => {
                redecode_at = None;
                let Some((name, config)) = &current else { continue };
                let decoder = decode::Consumer::new(&broadcast, config, name, decoding()).await;
                playing = start_playing(decoder, name, speaker, voice);
            }
            read = async { playing.as_mut().expect("guarded").decoder.read().await }, if playing.is_some() => {
                let Some(now_playing) = playing.as_mut() else { continue };
                match read {
                    Ok(Some(decode::Read::Frame(frame))) => {
                        redecode_after = REDECODE_AFTER;
                        if let Err(error) = now_playing.play(&frame.data, voice) {
                            debug!(error = %error, "a huddle peer's audio can't be played");
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
                        redecode_at = Some(tokio::time::Instant::now() + redecode_after);
                        redecode_after = (redecode_after * 2).min(REDECODE_AFTER_MOST);
                    }
                }
            }
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
        decoder,
        sink,
    })
}

struct Playing<K: Sink> {
    name: String,
    channels: u32,
    decoder: decode::Consumer,
    sink: K,
}

impl<K: Sink> Playing<K> {
    fn play(&mut self, data: &[u8], voice: &mut PeerVoice) -> anyhow::Result<()> {
        let samples = moq_audio::Format::F32.as_interleaved_f32(data, self.channels)?;
        voice.heard(rms(&samples));
        // Live audio that can't keep up is dropped rather than played late.
        let taken = self.sink.write(&samples)?;
        let given = samples.len() / self.channels.max(1) as usize;
        if taken < given {
            debug!(dropped = given - taken, "the speaker dropped late audio");
        }
        Ok(())
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
