//! One running call: publishes this device's voice at the call's path to
//! the listed peers only, plays each listed peer, and follows the list, the
//! mute switch and the devices as they change. The list is followed in a
//! task of its own, so a slow device never delays cutting someone off.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use iroh::PublicKey;
use iroh_live::{Audience, Node, Publication};
use shy_core::huddle::media::{DeviceChoice, DeviceList, MediaCall, MediaEvent, OutputState};
use tokio::{
    runtime::Handle,
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tracing::warn;

use crate::{
    access::{Access, Allowed, CallId},
    audience::{distinct_peers, peer_changes},
    backend::{Backend, Speaker},
    devices::{Resolved, ResolvedDevices, output_state, resolve, unchecked},
    listen::listen,
    listing::DeviceListing,
    mic::{Mic, until},
};

/// How often the devices are listed, to notice one plugged in or removed.
const DEVICE_CHECK_EVERY: Duration = Duration::from_secs(3);
/// How much of this device's voice is kept for a listener that falls
/// behind. A listener let in later can ask for no more than this from
/// before, so it stays well under a word.
const HISTORY: Duration = Duration::from_millis(200);

/// What a call needs from the media layer.
pub(crate) struct Shared<B: Backend> {
    pub(crate) backend: Arc<B>,
    pub(crate) node: Node,
    pub(crate) access: Access,
}

/// A started call. Dropping it ends the call at once: the publication is
/// withdrawn and every session is decided again.
pub(crate) struct Running<S: Speaker> {
    muted: watch::Sender<bool>,
    choice: watch::Sender<DeviceChoice>,
    speaker: watch::Receiver<Option<S>>,
    publication: Publication,
    node: Node,
    _peers: AbortOnDrop,
    _devices: AbortOnDrop,
}

impl<S: Speaker> Running<S> {
    pub(crate) fn set_muted(&self, muted: bool) {
        self.muted.send_replace(muted);
    }

    pub(crate) fn set_devices(&self, choice: DeviceChoice) {
        self.choice.send_replace(choice);
    }

    /// The call's speaker, once it's open.
    pub(crate) fn speaker(&self) -> Option<S> {
        self.speaker.borrow().clone()
    }
}

impl<S: Speaker> Drop for Running<S> {
    fn drop(&mut self) {
        self.publication.unpublish();
        self.node.reauthorize_all();
    }
}

/// Publishes at once, so a path that's taken fails the start; the rest
/// (devices, peers) starts in the background.
pub(crate) fn start<B: Backend>(
    shared: Shared<B>,
    call: MediaCall,
    runtime: &Handle,
) -> Result<Running<B::Speaker>> {
    let _entered = runtime.enter();
    let peers = distinct_peers(&call.my_path, &call.peers.borrow());
    let id = shared.access.open(Allowed {
        my_path: call.my_path.clone(),
        peers: peers.clone(),
    });
    let catalog = moq_mux::catalog::Config::default().with_max_age(HISTORY);
    let published = shared
        .node
        .publish_with(call.my_path.as_str(), audience(&peers), |broadcast| {
            moq_mux::catalog::Producer::new(&mut broadcast.clone(), catalog)
        })
        .context("publishing this device's voice");
    let (publication, catalog) = match published {
        Ok(published) => published,
        Err(error) => {
            shared.access.close();
            return Err(error);
        }
    };
    let (muted, muted_changes) = watch::channel(call.muted);
    let (choice, choice_changes) = watch::channel(call.devices.clone());
    let (speaker_sender, speaker) = watch::channel(None);
    let peers_task = runtime.spawn(follow_peers(PeerCall {
        id,
        node: shared.node.clone(),
        access: shared.access.clone(),
        my_path: call.my_path,
        peers: call.peers,
        publication: publication.clone(),
        events: call.events.clone(),
        speaker: speaker.clone(),
    }));
    let devices_task = runtime.spawn(run_devices(DeviceCall {
        backend: shared.backend,
        broadcast: publication.broadcast().clone(),
        catalog,
        events: call.events,
        muted: muted_changes,
        choice: choice_changes,
        speaker: speaker_sender,
    }));
    Ok(Running {
        muted,
        choice,
        speaker,
        publication,
        node: shared.node,
        _peers: AbortOnDrop(peers_task),
        _devices: AbortOnDrop(devices_task),
    })
}

fn audience(peers: &BTreeMap<PublicKey, String>) -> Audience {
    Audience::peers(peers.keys().copied())
}

struct PeerCall<S: Speaker> {
    id: CallId,
    node: Node,
    access: Access,
    my_path: String,
    peers: watch::Receiver<BTreeMap<PublicKey, String>>,
    publication: Publication,
    events: mpsc::Sender<MediaEvent>,
    speaker: watch::Receiver<Option<S>>,
}

/// Makes who may hear this device, and whom it plays, match the core's
/// list. Peers are played once the speaker is open.
async fn follow_peers<S: Speaker>(mut call: PeerCall<S>) {
    let mut allowed = distinct_peers(&call.my_path, &call.peers.borrow());
    let mut listeners = Listeners::default();
    loop {
        let wanted = distinct_peers(&call.my_path, &call.peers.borrow_and_update());
        call.access.set_peers(call.id, wanted.clone());
        call.publication.set_audience(audience(&wanted));
        let changes = peer_changes(&allowed, &wanted);
        let changed = changes
            .stop
            .into_iter()
            .chain(changes.start.into_iter().map(|(peer, _)| peer));
        for peer in changed {
            call.node.reauthorize(peer);
        }
        allowed = wanted;
        let speaker = call.speaker.borrow_and_update().clone();
        match &speaker {
            Some(speaker) => listeners.follow(&allowed, &call, speaker),
            None => listeners.0.clear(),
        }
        tokio::select! {
            changed = call.peers.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            changed = call.speaker.changed() => {
                if changed.is_err() {
                    break;
                }
            }
        }
    }
    // The core dropped the list or the call is ending: keep playing who's
    // there until the call is stopped.
    std::future::pending::<()>().await;
}

#[derive(Default)]
struct Listeners(BTreeMap<PublicKey, (String, AbortOnDrop)>);

impl Listeners {
    fn follow<S: Speaker>(&mut self, wanted: &BTreeMap<PublicKey, String>, call: &PeerCall<S>, speaker: &S) {
        let playing = self
            .0
            .iter()
            .map(|(peer, (path, _))| (*peer, path.clone()))
            .collect();
        let changes = peer_changes(&playing, wanted);
        for peer in changes.stop {
            self.0.remove(&peer);
        }
        for (peer, path) in changes.start {
            let task = tokio::spawn(listen(
                call.node.clone(),
                peer,
                path.clone(),
                speaker.clone(),
                call.events.clone(),
            ));
            self.0.insert(peer, (path, AbortOnDrop(task)));
        }
    }
}

struct DeviceCall<B: Backend> {
    backend: Arc<B>,
    broadcast: iroh_live::moq_net::broadcast::Producer,
    catalog: moq_mux::catalog::Producer,
    events: mpsc::Sender<MediaEvent>,
    muted: watch::Receiver<bool>,
    choice: watch::Receiver<DeviceChoice>,
    speaker: watch::Sender<Option<B::Speaker>>,
}

/// Opens the speaker and the microphone and keeps them on the right devices.
async fn run_devices<B: Backend>(mut call: DeviceCall<B>) {
    let mut output = OutputReport::new(call.events.clone());
    let mut listing = DeviceListing::new(call.backend.clone());
    let mut offered = listing.list_now().await;
    let mut resolved = resolve_against(&call.choice.borrow_and_update(), offered.as_ref());
    let speaker = loop {
        if let Some(speaker) = open_speaker(&*call.backend, &mut resolved).await {
            break speaker;
        }
        output.set(OutputState::Lost);
        tokio::time::sleep(DEVICE_CHECK_EVERY).await;
        offered = listing.list_now().await;
        resolved = resolve_against(&call.choice.borrow(), offered.as_ref());
    };
    call.speaker.send_replace(Some(speaker.clone()));
    let mut mic = Mic::new(
        call.backend.clone(),
        call.broadcast.clone(),
        call.catalog.clone(),
        call.events.clone(),
    );
    if *call.muted.borrow_and_update() {
        mic.close();
    } else {
        mic.open(&speaker, resolved.input.device.clone(), resolved.input.is_lost);
    }
    // A speaker that stopped working after a switch, tried again each check.
    let mut speaker_lost = false;
    output.set(state_of(&resolved, offered.as_ref(), &mic, speaker_lost));
    let mut device_check = tokio::time::interval(DEVICE_CHECK_EVERY);
    device_check.tick().await;
    loop {
        let retry_at = mic.retry_at();
        tokio::select! {
            changed = call.muted.changed() => {
                if changed.is_err() {
                    return;
                }
                if *call.muted.borrow_and_update() {
                    mic.close();
                } else {
                    mic.open(&speaker, resolved.input.device.clone(), resolved.input.is_lost);
                }
            }
            changed = call.choice.changed() => {
                if changed.is_err() {
                    return;
                }
                let wanted = resolve_against(&call.choice.borrow_and_update(), offered.as_ref());
                speaker_lost = !apply_devices(&call, &speaker, &mut mic, &mut resolved, wanted, speaker_lost).await;
            }
            report = mic.next_report() => mic.handle(report),
            () = until(retry_at) => {
                // On the last list; a fresh one follows, without holding up the retry.
                resolved.input = resolve_against(&call.choice.borrow(), offered.as_ref()).input;
                mic.retry(&speaker, resolved.input.device.clone());
                listing.start();
            }
            _ = device_check.tick() => listing.start(),
            listed = listing.finished(), if listing.is_running() => {
                let Some(fresh) = listed else { continue };
                let is_changed = offered.as_ref() != Some(&fresh);
                if !is_changed && !speaker_lost {
                    continue;
                }
                offered = Some(fresh);
                if is_changed {
                    let _ = call.events.try_send(MediaEvent::DevicesChanged);
                }
                let wanted = resolve_against(&call.choice.borrow(), offered.as_ref());
                speaker_lost = !apply_devices(&call, &speaker, &mut mic, &mut resolved, wanted, speaker_lost).await;
            }
        }
        output.set(state_of(&resolved, offered.as_ref(), &mic, speaker_lost));
    }
}

/// When the system couldn't say what's there, the choice is tried as is.
fn resolve_against(choice: &DeviceChoice, offered: Option<&DeviceList>) -> ResolvedDevices {
    match offered {
        Some(offered) => resolve(choice, offered),
        None => unchecked(choice),
    }
}

/// `Lost` only while no speaker plays: a chosen one that's gone while the
/// default plays is shown by the device pickers instead.
fn state_of<B: Backend>(
    resolved: &ResolvedDevices,
    offered: Option<&DeviceList>,
    mic: &Mic<B>,
    speaker_lost: bool,
) -> OutputState {
    if speaker_lost {
        return OutputState::Lost;
    }
    output_state(resolved, offered.unwrap_or(&DeviceList::default()), mic.is_open())
}

/// Moves to the devices `wanted` names, reopening only what changed, or
/// the speaker again when it was lost. Returns whether a speaker plays.
async fn apply_devices<B: Backend>(
    call: &DeviceCall<B>,
    speaker: &B::Speaker,
    mic: &mut Mic<B>,
    resolved: &mut ResolvedDevices,
    mut wanted: ResolvedDevices,
    speaker_lost: bool,
) -> bool {
    let speaker_plays = if wanted.output != resolved.output || speaker_lost {
        switch_speaker(speaker, &mut wanted.output).await
    } else {
        true
    };
    let input_changed = wanted.input != resolved.input;
    *resolved = wanted;
    if input_changed && !*call.muted.borrow() {
        mic.open(speaker, resolved.input.device.clone(), resolved.input.is_lost);
    }
    speaker_plays
}

/// Moves the speaker to `output`, or to the default when that won't open.
/// Returns whether either worked.
async fn switch_speaker<S: Speaker>(speaker: &S, output: &mut Resolved) -> bool {
    match speaker.switch(output.device.clone()).await {
        Ok(()) => return true,
        Err(error) => warn!(error = %error, "the speaker can't switch"),
    }
    if output.device.is_none() {
        return false;
    }
    match speaker.switch(None).await {
        Ok(()) => {
            *output = Resolved {
                device: None,
                is_lost: true,
            };
            true
        }
        Err(error) => {
            warn!(error = %error, "the default speaker can't open either");
            false
        }
    }
}

/// The chosen speaker, or the default when it won't open.
async fn open_speaker<B: Backend>(backend: &B, resolved: &mut ResolvedDevices) -> Option<B::Speaker> {
    let chosen = resolved.output.device.clone();
    match backend.open_speaker(chosen.clone()).await {
        Ok(speaker) => return Some(speaker),
        Err(error) => warn!(error = %error, "the speaker can't open"),
    }
    chosen.as_ref()?;
    resolved.output.device = None;
    resolved.output.is_lost = true;
    match backend.open_speaker(None).await {
        Ok(speaker) => Some(speaker),
        Err(error) => {
            warn!(error = %error, "the default speaker can't open either");
            None
        }
    }
}

/// Stops a task when dropped.
pub(crate) struct AbortOnDrop(pub(crate) JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Sends the speaker's state when it changes.
struct OutputReport {
    events: mpsc::Sender<MediaEvent>,
    last: Option<OutputState>,
}

impl OutputReport {
    fn new(events: mpsc::Sender<MediaEvent>) -> Self {
        Self { events, last: None }
    }

    fn set(&mut self, state: OutputState) {
        if self.last == Some(state) {
            return;
        }
        self.last = Some(state);
        let _ = self.events.try_send(MediaEvent::Output(state));
    }
}
