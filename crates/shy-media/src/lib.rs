//! Huddle audio: publishes this device's microphone to exactly the peers
//! the core lists, plays theirs mixed on the chosen speaker, and reports how
//! it's going. The core decides who may hear whom (`shy_core::huddle`); this
//! crate only follows its list. See "Voice huddles" in docs/decisions.md.

mod access;
mod audience;
mod backend;
mod call;
mod cues;
mod devices;
mod listen;
mod mic;
mod playout;
mod silence;
mod speaking;
mod system;
#[cfg(test)]
mod tests;
mod wav;

use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Result, bail};
use iroh::{Endpoint, protocol::RouterBuilder};
use iroh_live::{Node, NodeConfig};
use shy_core::{
    ExtraProtocols,
    huddle::media::{Cue, DeviceChoice, DeviceList, HuddleMedia, MediaCall, Ringing},
};
use tokio::runtime::Handle;
use tracing::{debug, warn};

use crate::{
    access::Access,
    backend::{Backend, Sink, Speaker},
    call::{AbortOnDrop, Running, Shared},
    system::System,
};

/// Huddle audio on this device's microphones and speakers. Make one per app
/// and hand it to every workspace it opens: [`protocols`](Self::protocols)
/// to serve media on the workspace's endpoint, and
/// [`huddle_media`](Self::huddle_media) for the core to drive.
#[derive(Clone)]
pub struct ShyMedia(Arc<Media<System>>);

impl ShyMedia {
    pub fn new() -> Self {
        Self(Arc::new(Media::new(System)))
    }

    /// The media protocol, admitted for workspace members only, with the
    /// policy that lets just the running call's listed peers in.
    pub fn protocols(&self) -> ExtraProtocols {
        self.0.protocols()
    }

    pub fn huddle_media(&self) -> Arc<dyn HuddleMedia> {
        self.0.clone()
    }
}

impl Default for ShyMedia {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct Media<B: Backend> {
    backend: Arc<B>,
    access: Access,
    /// The node on the open workspace's endpoint, and the runtime it runs on.
    attached: Mutex<Option<(Node, Handle)>>,
    running: Mutex<Option<Running<B::Speaker>>>,
    choice: Mutex<DeviceChoice>,
}

impl<B: Backend> Media<B> {
    pub(crate) fn new(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            access: Access::default(),
            attached: Mutex::new(None),
            running: Mutex::new(None),
            choice: Mutex::new(DeviceChoice::default()),
        }
    }

    /// Each workspace that opens mounts a new node on its endpoint, which
    /// replaces the last one's.
    pub(crate) fn protocols(self: &Arc<Self>) -> ExtraProtocols {
        let media = self.clone();
        ExtraProtocols::new(move |endpoint, router| media.attach(endpoint, router))
    }

    /// Runs inside the runtime that binds `endpoint`.
    pub(crate) fn attach(&self, endpoint: &Endpoint, router: RouterBuilder) -> RouterBuilder {
        let node = Node::new(
            endpoint.clone(),
            NodeConfig::default().with_policy(self.access.policy()),
        );
        *lock(&self.attached) = Some((node.clone(), Handle::current()));
        node.mount(router)
    }

    fn attached(&self) -> Option<(Node, Handle)> {
        lock(&self.attached).clone()
    }
}

impl<B: Backend> HuddleMedia for Media<B> {
    fn start(&self, call: MediaCall) -> Result<()> {
        let Some((node, runtime)) = self.attached() else {
            bail!("huddle audio isn't attached to a workspace's network");
        };
        *lock(&self.choice) = call.devices.clone();
        let shared = Shared {
            backend: self.backend.clone(),
            node,
            access: self.access.clone(),
        };
        let running = call::start(shared, call, &runtime)?;
        *lock(&self.running) = Some(running);
        Ok(())
    }

    /// Cuts every peer off at once: ending the call withdraws its
    /// publication and decides every session again.
    fn stop(&self) {
        self.access.close();
        lock(&self.running).take();
    }

    fn set_muted(&self, muted: bool) -> Result<()> {
        if let Some(running) = lock(&self.running).as_ref() {
            running.set_muted(muted);
        }
        Ok(())
    }

    fn set_devices(&self, choice: DeviceChoice) -> Result<()> {
        *lock(&self.choice) = choice.clone();
        if let Some(running) = lock(&self.running).as_ref() {
            running.set_devices(choice);
        }
        Ok(())
    }

    fn list_devices(&self) -> Result<DeviceList> {
        self.backend.list_devices()
    }

    fn play_cue(&self, cue: Cue) {
        let Some(sound) = self.sound_setup() else {
            debug!(?cue, "the huddle sound is skipped: no workspace network is up");
            return;
        };
        sound.runtime.clone().spawn(async move {
            if let Err(error) = sound.play_cue(cue).await {
                warn!(?cue, error = %error, "a huddle sound can't play");
            }
        });
    }

    fn ring(&self) -> Ringing {
        let Some(sound) = self.sound_setup() else {
            debug!("the ring is skipped: no workspace network is up");
            return Ringing::new(());
        };
        let task = sound.runtime.clone().spawn(async move {
            if let Err(error) = sound.ring().await {
                warn!(error = %error, "the ring can't play");
            }
        });
        Ringing::new(AbortOnDrop(task))
    }
}

impl<B: Backend> Media<B> {
    /// What a sound needs, unless no workspace is open or its network is
    /// closing (the app closing plays the leave sound as it shuts down).
    fn sound_setup(&self) -> Option<SoundSetup<B>> {
        let (node, runtime) = self.attached()?;
        if node.endpoint().is_closed() {
            return None;
        }
        Some(SoundSetup {
            backend: self.backend.clone(),
            speaker: lock(&self.running).as_ref().and_then(Running::speaker),
            device: lock(&self.choice).output.clone(),
            runtime,
        })
    }
}

struct SoundSetup<B: Backend> {
    backend: Arc<B>,
    /// The call's speaker, when a call is running.
    speaker: Option<B::Speaker>,
    /// The chosen output, for a sound outside a call.
    device: Option<String>,
    runtime: Handle,
}

impl<B: Backend> SoundSetup<B> {
    async fn play_cue(self, cue: Cue) -> Result<()> {
        let samples = cues::samples(cue)?;
        let mut sink = self.sink().await?;
        playout::play(&mut sink, &samples).await?;
        sink.finish().await;
        Ok(())
    }

    /// Loops the ring until the task is stopped.
    async fn ring(self) -> Result<()> {
        let cycle = cues::ring()?;
        let mut sink = self.sink().await?;
        loop {
            playout::play(&mut sink, &cycle).await?;
        }
    }

    /// A stream on the call's speaker, or on one opened for the sound alone.
    async fn sink(&self) -> Result<<B::Speaker as Speaker>::Sink> {
        let speaker = match &self.speaker {
            Some(speaker) => speaker.clone(),
            None => match self.backend.open_speaker(self.device.clone()).await {
                Ok(speaker) => speaker,
                Err(error) if self.device.is_some() => {
                    debug!(error = %error, "the chosen speaker can't open, so the sound plays on the default");
                    self.backend.open_speaker(None).await?
                }
                Err(error) => return Err(error),
            },
        };
        speaker.sink(wav::RATE, moq_audio::Layout::Mono)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .expect("huddle media locks are never held across a panic")
}
