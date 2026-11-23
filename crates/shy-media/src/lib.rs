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
mod listing;
mod mic;
mod playout;
mod silence;
mod speaking;
mod system;
#[cfg(test)]
mod tests;
mod wav;

use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result};
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

/// Huddle audio on this device's microphones and speakers. Make one per app,
/// and one [`WorkspaceAudio`] from it for each workspace that opens.
#[derive(Clone)]
pub struct ShyMedia(Arc<Media<System>>);

impl ShyMedia {
    pub fn new() -> Self {
        Self(Arc::new(Media::new(System)))
    }

    pub fn for_workspace(&self) -> WorkspaceAudio {
        WorkspaceAudio(Arc::new(WorkspaceMedia::new(self.0.clone())))
    }
}

impl Default for ShyMedia {
    fn default() -> Self {
        Self::new()
    }
}

/// The device's huddle audio as one workspace uses it: served on that
/// workspace's own endpoint, so its call, sounds and ring use its network
/// and its device key, and only its call can be stopped or muted from it.
#[derive(Clone)]
pub struct WorkspaceAudio(Arc<WorkspaceMedia<System>>);

impl WorkspaceAudio {
    /// The media protocol, admitted for workspace members only, with the
    /// policy that lets just the running call's listed peers in.
    pub fn protocols(&self) -> ExtraProtocols {
        self.0.protocols()
    }

    pub fn huddle_media(&self) -> Arc<dyn HuddleMedia> {
        self.0.clone()
    }
}

/// What the device has once, whatever the workspace: its audio devices and
/// the one call it can be in.
pub(crate) struct Media<B: Backend> {
    backend: Arc<B>,
    active: Mutex<Option<Active<B::Speaker>>>,
    choice: Mutex<DeviceChoice>,
    workspaces_made: AtomicU64,
}

/// The running call and the workspace it runs in.
struct Active<S: Speaker> {
    workspace: u64,
    access: Access,
    running: Running<S>,
}

impl<S: Speaker> Active<S> {
    /// Cuts every peer off at once: closing the access, then withdrawing the
    /// publication, decides every session again.
    fn end(self) {
        self.access.close();
        drop(self.running);
    }
}

impl<B: Backend> Media<B> {
    pub(crate) fn new(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            active: Mutex::new(None),
            choice: Mutex::new(DeviceChoice::default()),
            workspaces_made: AtomicU64::new(0),
        }
    }

    /// Ends the running call if `workspace` holds it, or any call when
    /// `workspace` is `None`.
    fn end_call(&self, workspace: Option<u64>) {
        let ended = {
            let mut active = lock(&self.active);
            let is_ending = active
                .as_ref()
                .is_some_and(|call| workspace.is_none_or(|workspace| call.workspace == workspace));
            if is_ending { active.take() } else { None }
        };
        if let Some(call) = ended {
            call.end();
        }
    }
}

pub(crate) struct WorkspaceMedia<B: Backend> {
    media: Arc<Media<B>>,
    id: u64,
    /// Who may connect to this workspace's node: the peers of its call.
    access: Access,
    /// The node on this workspace's endpoint, and the runtime it runs on.
    attached: Mutex<Option<(Node, Handle)>>,
}

impl<B: Backend> WorkspaceMedia<B> {
    pub(crate) fn new(media: Arc<Media<B>>) -> Self {
        let id = media.workspaces_made.fetch_add(1, Ordering::Relaxed);
        Self {
            media,
            id,
            access: Access::default(),
            attached: Mutex::new(None),
        }
    }

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

    /// Runs `change` on the running call if it is this workspace's.
    fn with_my_call(&self, change: impl FnOnce(&Running<B::Speaker>)) {
        if let Some(call) = lock(&self.media.active)
            .as_ref()
            .filter(|call| call.workspace == self.id)
        {
            change(&call.running);
        }
    }
}

impl<B: Backend> HuddleMedia for WorkspaceMedia<B> {
    /// Ends any call first, whichever workspace holds it: one microphone,
    /// so one call at a time.
    fn start(&self, call: MediaCall) -> Result<()> {
        let (node, runtime) = self
            .attached()
            .context("huddle audio isn't attached to this workspace's network")?;
        self.media.end_call(None);
        *lock(&self.media.choice) = call.devices.clone();
        let shared = Shared {
            backend: self.media.backend.clone(),
            node,
            access: self.access.clone(),
        };
        let running = call::start(shared, call, &runtime)?;
        let replaced = lock(&self.media.active).replace(Active {
            workspace: self.id,
            access: self.access.clone(),
            running,
        });
        if let Some(replaced) = replaced {
            replaced.end();
        }
        Ok(())
    }

    fn stop(&self) {
        self.media.end_call(Some(self.id));
    }

    fn set_muted(&self, muted: bool) -> Result<()> {
        self.with_my_call(|running| running.set_muted(muted));
        Ok(())
    }

    /// The devices are the device's, so the choice applies to the call
    /// wherever it is.
    fn set_devices(&self, choice: DeviceChoice) -> Result<()> {
        *lock(&self.media.choice) = choice.clone();
        if let Some(call) = lock(&self.media.active).as_ref() {
            call.running.set_devices(choice);
        }
        Ok(())
    }

    fn list_devices(&self) -> Result<DeviceList> {
        self.media.backend.list_devices()
    }

    fn play_cue(&self, cue: Cue) {
        let Some(sound) = self.sound_setup() else {
            debug!(
                ?cue,
                "the huddle sound is skipped: this workspace's network is down"
            );
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
            debug!("the ring is skipped: this workspace's network is down");
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

impl<B: Backend> WorkspaceMedia<B> {
    /// What a sound needs, unless this workspace's network isn't up or is
    /// closing (the app closing plays the leave sound as it shuts down).
    fn sound_setup(&self) -> Option<SoundSetup<B>> {
        let (node, runtime) = self.attached()?;
        if node.endpoint().is_closed() {
            return None;
        }
        Some(SoundSetup {
            backend: self.media.backend.clone(),
            speaker: lock(&self.media.active)
                .as_ref()
                .and_then(|call| call.running.speaker()),
            device: lock(&self.media.choice).output.clone(),
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
