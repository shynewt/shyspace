//! The system's own microphones and speakers, through moq-audio (cpal):
//! PipeWire or ALSA on Linux, CoreAudio on macOS, WASAPI on Windows.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use moq_audio::{Layout, aec, capture, encode, playback};
use shy_core::huddle::media::DeviceList;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;

use crate::{
    backend::{Backend, Outgoing, Sink, Speaker, VoiceFailure, VoiceReport},
    devices::{Listed, microphones, offer},
};

/// How often an open microphone reports its level.
const LEVEL_EVERY: Duration = Duration::from_millis(50);
/// How long a new microphone waits for the one it replaces to let go of the
/// echo canceller, which takes one microphone at a time.
const CANCELLER_WAIT: Duration = Duration::from_secs(2);
const CANCELLER_RETRY: Duration = Duration::from_millis(50);

#[derive(Debug, Default)]
pub(crate) struct System;

impl Backend for System {
    type Speaker = SystemSpeaker;
    type Voice = SystemVoice;

    fn list_devices(&self) -> Result<DeviceList> {
        // moq-audio lists on a blocking task of a Tokio runtime, and this may
        // be called from inside one, so it gets a runtime of its own.
        let (inputs, outputs) = std::thread::spawn(|| -> Result<_> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .context("starting the audio device lister")?;
            runtime.block_on(async {
                let inputs = capture::devices().await.context("listing microphones")?;
                let outputs = playback::devices().await.context("listing speakers")?;
                Ok((inputs, outputs))
            })
        })
        .join()
        .map_err(|_| anyhow!("listing audio devices crashed"))??;
        let outputs: Vec<Listed> = outputs
            .into_iter()
            .map(|device| Listed {
                id: device.id,
                name: device.name,
                is_default: device.default,
                host: device.host,
            })
            .collect();
        let inputs = inputs
            .into_iter()
            .map(|device| Listed {
                id: device.id,
                name: device.name,
                is_default: device.default,
                host: device.host,
            })
            .collect();
        Ok(DeviceList {
            inputs: offer(microphones(inputs, &outputs)),
            outputs: offer(outputs),
        })
    }

    async fn open_speaker(&self, device: Option<String>) -> Result<SystemSpeaker> {
        let mut config = playback::Config::default();
        config.device = device;
        let engine = playback::Engine::open(config)
            .await
            .context("opening the speaker")?;
        let canceller = engine
            .canceller(aec::Config::default())
            .context("starting echo cancellation")?;
        Ok(SystemSpeaker { engine, canceller })
    }

    fn open_voice(
        &self,
        speaker: &SystemSpeaker,
        device: Option<String>,
        outgoing: Outgoing,
        reports: mpsc::Sender<VoiceReport>,
    ) -> Result<SystemVoice> {
        let mut config = capture::Config::default();
        config.source = capture::Source::Microphone(device);
        config.aec = Some(speaker.canceller.clone());
        let (stop, stopped) = oneshot::channel();
        // On macOS the capture holds Objective-C handles across awaits, so it
        // gets a thread and a runtime of its own.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("starting the microphone's runtime")?;
        std::thread::Builder::new()
            .name("shy-media-microphone".into())
            .spawn(move || {
                runtime.block_on(async move {
                    tokio::select! {
                        () = run_voice(config, outgoing, &reports) => {}
                        _ = stopped => {}
                    }
                });
            })
            .context("starting the microphone's thread")?;
        Ok(SystemVoice { _stop: stop })
    }
}

/// Publishes the microphone until it fails; reports how it goes.
async fn run_voice(config: capture::Config, outgoing: Outgoing, reports: &mpsc::Sender<VoiceReport>) {
    let failure = match publish_voice(config, outgoing, reports).await {
        Ok(()) => return,
        Err(error) => {
            debug!(error = %error, "the microphone stopped");
            failure_of(&error)
        }
    };
    let _ = reports.try_send(VoiceReport::Failed(failure));
}

async fn publish_voice(
    config: capture::Config,
    outgoing: Outgoing,
    reports: &mpsc::Sender<VoiceReport>,
) -> Result<(), moq_audio::Error> {
    let microphone = open_microphone(config).await?;
    let mut options = encode::Capture::new(microphone.consume());
    options.encode.track = Some(outgoing.track);
    // Encoding only for listeners would first send them what was said
    // before they could hear, so the voice is encoded as it's spoken.
    options.retain = false;
    let (publication, driver) = encode::publish_capture(outgoing.broadcast, outgoing.catalog, options)?;
    let _ = reports.try_send(VoiceReport::Opened);
    let levels = async {
        let mut every = tokio::time::interval(LEVEL_EVERY);
        loop {
            every.tick().await;
            let level = publication.level();
            let _ = reports.try_send(VoiceReport::Level {
                rms: level.rms(),
                peak: level.peak(),
            });
        }
    };
    tokio::select! {
        result = microphone.run() => result,
        result = driver.run() => result,
        () = levels => Ok(()),
    }
}

/// Retries while the microphone this one replaces still holds the echo
/// canceller: it lets go once its thread winds down.
async fn open_microphone(config: capture::Config) -> Result<capture::Producer, moq_audio::Error> {
    let deadline = tokio::time::Instant::now() + CANCELLER_WAIT;
    loop {
        match capture::Producer::open(config.clone()).await {
            Err(moq_audio::Error::Busy(_)) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(CANCELLER_RETRY).await;
            }
            opened => return opened,
        }
    }
}

/// moq-audio says a refused permission only in its message (macOS).
fn failure_of(error: &moq_audio::Error) -> VoiceFailure {
    match error {
        moq_audio::Error::Capture(message) if message.contains("access denied") => {
            VoiceFailure::PermissionDenied
        }
        _ => VoiceFailure::Device,
    }
}

/// Dropping it ends the microphone's thread, which closes the device.
#[derive(Debug)]
pub(crate) struct SystemVoice {
    _stop: oneshot::Sender<()>,
}

#[derive(Clone)]
pub(crate) struct SystemSpeaker {
    engine: playback::Engine,
    /// One per speaker: it learns this speaker's echo, whichever microphone
    /// is open.
    canceller: aec::Control,
}

impl Speaker for SystemSpeaker {
    type Sink = SystemSink;

    fn sink(&self, sample_rate: u32, layout: Layout) -> Result<SystemSink> {
        let mut input = playback::Input::default();
        input.format = moq_audio::Format::F32;
        input.sample_rate = sample_rate;
        input.layout = layout;
        let sink = self
            .engine
            .sink(input)
            .context("adding a stream to the speaker")?;
        Ok(SystemSink(sink))
    }

    async fn switch(&self, device: Option<String>) -> Result<()> {
        let mut config = playback::Config::default();
        config.device = device;
        self.engine.switch(config).await.context("switching the speaker")
    }
}

pub(crate) struct SystemSink(playback::Sink);

impl Sink for SystemSink {
    fn write(&mut self, samples: &[f32]) -> Result<()> {
        let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_ne_bytes()).collect();
        let written = self.0.write(&bytes).context("playing audio")?;
        // Live audio that can't keep up is dropped rather than played late.
        if written.dropped_sample_frames > 0 {
            debug!(
                dropped = written.dropped_sample_frames,
                "the speaker dropped late audio"
            );
        }
        Ok(())
    }

    async fn finish(self) {
        self.0.finish().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_microphone_is_told_from_a_broken_one() {
        let refused = moq_audio::Error::Capture(
            "microphone access denied; grant it in System Settings > Privacy & Security > Microphone".into(),
        );
        assert_eq!(failure_of(&refused), VoiceFailure::PermissionDenied);
        let gone = moq_audio::Error::Device("input device \"usb\" not found".into());
        assert_eq!(failure_of(&gone), VoiceFailure::Device);
    }
}
