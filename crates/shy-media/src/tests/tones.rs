//! A backend with no audio devices: each "microphone" plays a steady tone,
//! and the "speaker" keeps everything it's given, one recording per stream.

use std::{
    f32::consts::TAU,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::Result;
use bytes::Bytes;
use moq_audio::{Frame, Layout, encode};
use shy_core::huddle::media::{AudioDevice, DeviceList};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{
    backend::{Backend, Outgoing, Sink, Speaker, VoiceReport},
    listen::rms,
};

const RATE: u32 = 48_000;
const FRAME: Duration = Duration::from_millis(20);
const LOUDNESS: f32 = 0.3;

pub(super) struct Tones {
    frequency: f32,
    speaker: Recorder,
    /// Every microphone fails to open, as a broken one would.
    is_microphone_broken: bool,
    /// The microphone sends one packet that won't decode, a second in.
    sends_a_bad_packet: bool,
    listings: Listings,
}

/// Device listings: how many started, and whether they hang (as a sound
/// server that stopped answering would) until the test lets them go.
#[derive(Clone, Default)]
pub(super) struct Listings {
    started: Arc<AtomicUsize>,
    /// Listings after this many hang; `None` never hangs.
    hang_after: Option<usize>,
    released: Arc<(Mutex<bool>, Condvar)>,
}

impl Listings {
    pub(super) fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    /// Lets every hanging listing finish, so no test thread outlives its test.
    pub(super) fn release(&self) {
        let (released, wake) = &*self.released;
        *released.lock().expect("the release flag is never poisoned") = true;
        wake.notify_all();
    }

    fn wait_if_hanging(&self) {
        let count = self.started.fetch_add(1, Ordering::SeqCst) + 1;
        if self.hang_after.is_none_or(|after| count <= after) {
            return;
        }
        let (released, wake) = &*self.released;
        let mut is_released = released.lock().expect("the release flag is never poisoned");
        while !*is_released {
            is_released = wake
                .wait(is_released)
                .expect("the release flag is never poisoned");
        }
    }
}

impl Tones {
    pub(super) fn new(frequency: f32) -> Self {
        Self {
            frequency,
            speaker: Recorder::default(),
            is_microphone_broken: false,
            sends_a_bad_packet: false,
            listings: Listings::default(),
        }
    }

    pub(super) fn with_a_bad_packet(frequency: f32) -> Self {
        Self {
            sends_a_bad_packet: true,
            ..Self::new(frequency)
        }
    }

    /// The first `after` device listings answer; every later one hangs.
    pub(super) fn with_listings_hanging_after(frequency: f32, after: usize) -> Self {
        Self {
            listings: Listings {
                hang_after: Some(after),
                ..Listings::default()
            },
            ..Self::new(frequency)
        }
    }

    pub(super) fn listings(&self) -> Listings {
        self.listings.clone()
    }

    pub(super) fn with_a_broken_microphone(frequency: f32) -> Self {
        Self {
            is_microphone_broken: true,
            ..Self::new(frequency)
        }
    }

    pub(super) fn speaker(&self) -> Recorder {
        self.speaker.clone()
    }
}

impl Backend for Tones {
    type Speaker = Recorder;
    type Voice = Playing;

    fn list_devices(&self) -> Result<DeviceList> {
        self.listings.wait_if_hanging();
        let device = |id: &str| AudioDevice {
            id: id.into(),
            name: id.into(),
            is_default: true,
            is_bluetooth: false,
        };
        Ok(DeviceList {
            inputs: vec![device("tone")],
            outputs: vec![device("recorder")],
        })
    }

    async fn open_speaker(&self, _device: Option<String>) -> Result<Recorder> {
        Ok(self.speaker.clone())
    }

    fn open_voice(
        &self,
        _speaker: &Recorder,
        _device: Option<String>,
        outgoing: Outgoing,
        reports: mpsc::Sender<VoiceReport>,
    ) -> Result<Playing> {
        anyhow::ensure!(!self.is_microphone_broken, "the microphone is broken");
        let frequency = self.frequency;
        let sends_a_bad_packet = self.sends_a_bad_packet;
        Ok(Playing(tokio::spawn(async move {
            let played = match sends_a_bad_packet {
                true => play_tone_with_a_bad_packet(outgoing, frequency, &reports).await,
                false => play_tone(outgoing, frequency, &reports).await,
            };
            if let Err(error) = played {
                panic!("the test tone stopped: {error:#}");
            }
        })))
    }
}

/// Stops the tone when dropped, as closing a microphone would.
pub(super) struct Playing(JoinHandle<()>);

impl Drop for Playing {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn play_tone(outgoing: Outgoing, frequency: f32, reports: &mpsc::Sender<VoiceReport>) -> Result<()> {
    let mut broadcast = outgoing.broadcast;
    let clock = outgoing.catalog.clock();
    let mut options = encode::Options::default();
    options.track = Some(outgoing.track);
    let input = encode::Input::new(RATE, Layout::Mono);
    let mut producer = encode::Producer::new(&mut broadcast, outgoing.catalog, input, &options)?;
    let _ = reports.try_send(VoiceReport::Opened);
    let per_frame = (RATE as f32 * FRAME.as_secs_f32()) as usize;
    let mut every = real_time(FRAME);
    for frame in 0.. {
        every.tick().await;
        let samples: Vec<f32> = (0..per_frame)
            .map(|index| {
                let time = (frame * per_frame + index) as f32 / RATE as f32;
                LOUDNESS * (TAU * frequency * time).sin()
            })
            .collect();
        let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_ne_bytes()).collect();
        producer.write(&Frame::new(Bytes::from(bytes), clock.now()))?;
        let _ = reports.try_send(VoiceReport::Level {
            rms: rms(&samples),
            peak: LOUDNESS,
        });
    }
    Ok(())
}

/// The tone, encoded and published packet by packet as moq-audio's own
/// producer would, except that one packet a second in is garbage.
async fn play_tone_with_a_bad_packet(
    outgoing: Outgoing,
    frequency: f32,
    reports: &mpsc::Sender<VoiceReport>,
) -> Result<()> {
    const BAD_FRAME: usize = 50;
    const FRAMES_PER_GROUP: usize = 10;
    let broadcast = outgoing.broadcast;
    let clock = outgoing.catalog.clock();
    let track = broadcast.create_track(outgoing.track, outgoing.catalog.track_info(0))?;
    let mut track = outgoing.catalog.audio(
        track,
        moq_mux::container::legacy::Wire(moq_mux::container::Kind::Audio),
        None,
    )?;
    let mut encoder = encode::Encoder::new(&encode::Config::new(RATE, Layout::Mono))?;
    track.set(encoder.catalog())?;
    let _ = reports.try_send(VoiceReport::Opened);
    let per_frame = encoder.frame_size();
    let mut every = real_time(Duration::from_secs_f64(per_frame as f64 / f64::from(RATE)));
    for frame in 0.. {
        every.tick().await;
        let samples: Vec<f32> = (0..per_frame)
            .map(|index| {
                let time = (frame * per_frame + index) as f32 / RATE as f32;
                LOUDNESS * (TAU * frequency * time).sin()
            })
            .collect();
        encoder.encode(&samples)?;
        while let Some(packet) = encoder.receive()? {
            let payload = match frame == BAD_FRAME {
                true => Bytes::from_static(&[0xff, 0xff, 0xff, 0xff]),
                false => packet.payload,
            };
            let keyframe = track.needs_keyframe();
            track.write(moq_mux::container::Frame {
                timestamp: clock.now(),
                payload,
                keyframe,
                duration: None,
            })?;
        }
        if frame % FRAMES_PER_GROUP == FRAMES_PER_GROUP - 1 {
            track.cut(Some(clock.now()))?;
        }
    }
    Ok(())
}

/// A tick per frame that never catches up in a burst after a stall, as a
/// microphone doesn't: the tone's audio never runs ahead of the clock.
fn real_time(frame: Duration) -> tokio::time::Interval {
    let mut every = tokio::time::interval(frame);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    every
}

/// One stream's samples, as played.
type Stream = Arc<Mutex<Vec<f32>>>;

/// Every stream the speaker was given.
#[derive(Debug, Clone, Default)]
pub(super) struct Recorder(Arc<Mutex<Vec<Stream>>>);

impl Recorder {
    /// The samples of each stream so far.
    pub(super) fn streams(&self) -> Vec<Vec<f32>> {
        self.0
            .lock()
            .expect("recorder lock")
            .iter()
            .map(|stream| stream.lock().expect("stream lock").clone())
            .collect()
    }

    /// How many samples it was given in all.
    pub(super) fn total(&self) -> usize {
        self.streams().iter().map(Vec::len).sum()
    }
}

impl Speaker for Recorder {
    type Sink = Recording;

    fn sink(&self, sample_rate: u32, layout: Layout) -> Result<Recording> {
        assert_eq!(
            sample_rate, RATE,
            "Opus decodes at the rate the tones are made at"
        );
        let stream = Arc::new(Mutex::new(Vec::new()));
        self.0.lock().expect("recorder lock").push(stream.clone());
        Ok(Recording {
            stream,
            channels: layout.channels() as usize,
        })
    }

    async fn switch(&self, _device: Option<String>) -> Result<()> {
        Ok(())
    }
}

/// Keeps the first channel: every channel of a tone carries the same wave.
pub(super) struct Recording {
    stream: Stream,
    channels: usize,
}

impl Sink for Recording {
    fn write(&mut self, samples: &[f32]) -> Result<usize> {
        let first_channel = samples.iter().step_by(self.channels).copied();
        self.stream.lock().expect("stream lock").extend(first_channel);
        Ok(samples.len() / self.channels)
    }

    async fn finish(self) {}
}

/// Roughly the pitch of `samples`, from how often the wave crosses zero.
pub(super) fn pitch(samples: &[f32]) -> f32 {
    let crossings = samples
        .windows(2)
        .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
        .count();
    crossings as f32 / 2.0 / (samples.len() as f32 / RATE as f32)
}
