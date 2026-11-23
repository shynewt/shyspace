//! The microphone side of a call. Muting closes the device, so the system's
//! microphone indicator goes dark; unmuting opens it again. Reports the
//! microphone's state and this device's own speaking.

use std::{sync::Arc, time::Duration};

use shy_core::huddle::media::{MediaEvent, MicState};
use tokio::{sync::mpsc, time::Instant};
use tracing::warn;

use crate::{
    backend::{Backend, Outgoing, VoiceFailure, VoiceReport},
    silence::SilenceWatch,
    speaking::SpeakingDetector,
};

/// How long a microphone that failed waits before it's tried again.
const RETRY_AFTER: Duration = Duration::from_secs(3);
const REPORTS: usize = 64;

pub(crate) struct Mic<B: Backend> {
    backend: Arc<B>,
    broadcast: iroh_live::moq_net::broadcast::Producer,
    catalog: moq_mux::catalog::Producer,
    events: mpsc::Sender<MediaEvent>,
    openings: u64,
    open: Option<Open<B>>,
    state: Option<MicState>,
    me: SpeakingDetector,
    retry_at: Option<Instant>,
}

struct Open<B: Backend> {
    _voice: B::Voice,
    reports: mpsc::Receiver<VoiceReport>,
    silence: SilenceWatch,
}

impl<B: Backend> Mic<B> {
    pub(crate) fn new(
        backend: Arc<B>,
        broadcast: iroh_live::moq_net::broadcast::Producer,
        catalog: moq_mux::catalog::Producer,
        events: mpsc::Sender<MediaEvent>,
    ) -> Self {
        Self {
            backend,
            broadcast,
            catalog,
            events,
            openings: 0,
            open: None,
            state: None,
            me: SpeakingDetector::default(),
            retry_at: None,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Opens `device` (`None` is the default), first saying the chosen one
    /// is gone when `is_lost`.
    pub(crate) fn open(&mut self, speaker: &B::Speaker, device: Option<String>, is_lost: bool) {
        self.release();
        self.retry_at = None;
        if is_lost {
            self.report(MicState::Lost);
        }
        self.report(MicState::Opening);
        self.start(speaker, device);
    }

    /// Tries a failed microphone again. It stays reported lost until it
    /// works, rather than flickering through opening on every try.
    pub(crate) fn retry(&mut self, speaker: &B::Speaker, device: Option<String>) {
        self.release();
        self.retry_at = None;
        self.start(speaker, device);
    }

    fn start(&mut self, speaker: &B::Speaker, device: Option<String>) {
        self.openings += 1;
        let outgoing = Outgoing {
            broadcast: self.broadcast.clone(),
            catalog: self.catalog.clone(),
            track: format!("audio-{}", self.openings),
        };
        let (sender, reports) = mpsc::channel(REPORTS);
        match self.backend.open_voice(speaker, device, outgoing, sender) {
            Ok(voice) => {
                self.open = Some(Open {
                    _voice: voice,
                    reports,
                    silence: SilenceWatch::opened(std::time::Instant::now()),
                });
            }
            Err(error) => {
                warn!(error = %error, "the microphone can't start");
                self.failed(VoiceFailure::Device);
            }
        }
    }

    /// Closes the device: muted.
    pub(crate) fn close(&mut self) {
        self.release();
        self.retry_at = None;
        self.report(MicState::Off);
    }

    /// The next report from the open microphone; never resolves while none is open.
    pub(crate) async fn next_report(&mut self) -> VoiceReport {
        let Some(open) = self.open.as_mut() else {
            return std::future::pending().await;
        };
        open.reports
            .recv()
            .await
            .unwrap_or(VoiceReport::Failed(VoiceFailure::Device))
    }

    /// When a failed microphone should be tried again.
    pub(crate) fn retry_at(&self) -> Option<Instant> {
        self.retry_at
    }

    pub(crate) fn handle(&mut self, report: VoiceReport) {
        match report {
            VoiceReport::Opened => {}
            VoiceReport::Level { rms, peak } => self.heard(rms, peak),
            VoiceReport::Failed(failure) => {
                self.release();
                self.failed(failure);
            }
        }
    }

    fn heard(&mut self, rms: f32, peak: f32) {
        let now = std::time::Instant::now();
        let Some(open) = self.open.as_mut() else { return };
        if let Some(state) = open.silence.observe(peak, now) {
            self.report(state);
        }
        if let Some(speaking) = self.me.observe(rms, now) {
            let _ = self.events.try_send(MediaEvent::MeSpeaking(speaking));
        }
    }

    /// A refused permission waits for the person; anything else is tried
    /// again after a while.
    fn failed(&mut self, failure: VoiceFailure) {
        match failure {
            VoiceFailure::PermissionDenied => self.report(MicState::PermissionDenied),
            VoiceFailure::Device => {
                self.report(MicState::Lost);
                self.retry_at = Some(Instant::now() + RETRY_AFTER);
            }
        }
    }

    fn release(&mut self) {
        self.open = None;
        if self.me.stop() {
            let _ = self.events.try_send(MediaEvent::MeSpeaking(false));
        }
    }

    fn report(&mut self, state: MicState) {
        if self.state == Some(state) {
            return;
        }
        self.state = Some(state);
        let _ = self.events.try_send(MediaEvent::Mic(state));
    }
}

/// Resolves at `at`; never, when `None`.
pub(crate) async fn until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}
