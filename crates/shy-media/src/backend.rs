//! Where sound comes from and goes to. The real one opens the system's
//! microphones and speakers (`system`); tests use generated tones instead,
//! so two endpoints can talk without any audio device.

use std::future::Future;

use anyhow::Result;
use iroh_live::moq_net::broadcast;
use moq_audio::Layout;
use shy_core::huddle::media::DeviceList;
use tokio::sync::mpsc;

/// The broadcast this device's voice goes into, with its catalog: what a
/// listener reads to find the audio track.
#[derive(Clone)]
pub(crate) struct Outgoing {
    pub(crate) broadcast: broadcast::Producer,
    pub(crate) catalog: moq_mux::catalog::Producer,
    /// A fresh name per opening: a closed track's name isn't reused.
    pub(crate) track: String,
}

/// What an open voice reports.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum VoiceReport {
    /// The microphone opened and its audio is being published.
    Opened,
    /// The level of a short stretch of captured audio, after echo cancellation.
    Level { rms: f32, peak: f32 },
    /// The microphone couldn't open, or stopped.
    Failed(VoiceFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceFailure {
    /// The system refused access.
    PermissionDenied,
    /// The device is gone or broken.
    Device,
}

pub(crate) trait Backend: Send + Sync + 'static {
    type Speaker: Speaker;
    /// Dropping it closes the microphone.
    type Voice: Send + 'static;

    /// Blocks while the system lists its devices.
    fn list_devices(&self) -> Result<DeviceList>;

    /// Opens `device`, or the system default when `None`.
    fn open_speaker(&self, device: Option<String>) -> impl Future<Output = Result<Self::Speaker>> + Send;

    /// Starts publishing the microphone `device` into `outgoing`, removing
    /// what `speaker` plays from it. Returns at once; the opening's outcome
    /// arrives on `reports`.
    fn open_voice(
        &self,
        speaker: &Self::Speaker,
        device: Option<String>,
        outgoing: Outgoing,
        reports: mpsc::Sender<VoiceReport>,
    ) -> Result<Self::Voice>;
}

/// An open output device that mixes any number of sinks.
pub(crate) trait Speaker: Clone + Send + Sync + 'static {
    type Sink: Sink;

    fn sink(&self, sample_rate: u32, layout: Layout) -> Result<Self::Sink>;

    /// Moves every sink to `device`, or to the system default when `None`.
    fn switch(&self, device: Option<String>) -> impl Future<Output = Result<()>> + Send;
}

/// One stream into the speaker's mix.
pub(crate) trait Sink: Send + 'static {
    /// Takes interleaved samples in the layout the sink was made with.
    fn write(&mut self, samples: &[f32]) -> Result<()>;

    /// Waits until what was written has played.
    fn finish(self) -> impl Future<Output = ()> + Send;
}
