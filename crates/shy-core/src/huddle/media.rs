//! The boundary between the core's huddle rules and whatever moves the
//! sound. The core decides who is in a huddle and who may hear whom; an
//! implementation (the `shy-media` crate, a phone's native audio, a fake in
//! tests) only publishes this device's audio at its path to exactly the
//! peers it's given, and plays theirs. Plain owned types throughout, so a
//! foreign-language binding can implement it.

use std::collections::BTreeMap;

use anyhow::Result;
use iroh::PublicKey;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

/// What the core hands the media layer when this device joins a huddle.
pub struct MediaCall {
    /// Where this device publishes, from its session (`beacon::media_path_name`).
    pub my_path: String,
    /// Who may hear this device and whom it plays: each peer's device and
    /// media path. The core updates it as people join, leave or stop being
    /// members; the media layer follows it, and nobody else is offered audio.
    pub peers: watch::Receiver<BTreeMap<PublicKey, String>>,
    pub events: mpsc::Sender<MediaEvent>,
    pub muted: bool,
    pub devices: DeviceChoice,
}

/// The microphone and speaker to use; `None` means the system's default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceChoice {
    pub input: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// A Bluetooth headset: using its microphone drops it to call quality.
    pub is_bluetooth: bool,
}

/// The microphones and speakers the system offers right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceList {
    pub inputs: Vec<AudioDevice>,
    pub outputs: Vec<AudioDevice>,
}

/// How this device's link to one peer's media is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    Connecting,
    /// Connected straight to the peer.
    Direct,
    /// Connected through a relay: it works, but may sound worse.
    Relayed,
    Reconnecting,
}

/// The microphone's state, for telling the person what's wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MicState {
    /// Closed, because muted.
    Off,
    Opening,
    Live,
    /// The system refused access (macOS asks once per app).
    PermissionDenied,
    /// Open but delivering exact silence, which a real room never is.
    Silent,
    /// The device went away; the default is being tried.
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputState {
    Playing,
    /// A Bluetooth headset fell back to its call mode: mono, low quality.
    BluetoothCallMode,
    Lost,
}

/// What the media layer reports while in a huddle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaEvent {
    Link {
        peer: PublicKey,
        link: Link,
    },
    /// From the audio level of what arrives, held briefly so it doesn't flicker.
    Speaking {
        peer: PublicKey,
        speaking: bool,
    },
    /// This device's own voice, for its own indicator.
    MeSpeaking(bool),
    Mic(MicState),
    Output(OutputState),
    /// The peer's media can't be read by this version, or the other way round.
    Incompatible {
        peer: PublicKey,
    },
    /// A microphone or speaker was plugged in or removed.
    DevicesChanged,
}

/// A short sound for someone coming or going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// This device joined a huddle.
    Joined,
    PeerJoined,
    PeerLeft,
    /// This device left its huddle.
    Left,
}

/// Moves a huddle's sound. One call at a time.
///
/// `start` and `stop` run while the core holds its join/leave lock, so they
/// must return promptly and never wait for an event to be delivered: send
/// events with `try_send` and drop them when the channel is full.
pub trait HuddleMedia: Send + Sync + 'static {
    fn start(&self, call: MediaCall) -> Result<()>;
    fn stop(&self);
    fn set_muted(&self, muted: bool) -> Result<()>;
    fn set_devices(&self, choice: DeviceChoice) -> Result<()>;
    fn list_devices(&self) -> Result<DeviceList>;
    /// Plays `cue` on the chosen speaker, whether or not a call is running.
    /// Returns at once; a sound that can't play is logged, never an error,
    /// so it can't hold up or break a join or a leave.
    fn play_cue(&self, cue: Cue);
}

#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Default)]
    struct Live {
        peers: Option<watch::Receiver<BTreeMap<PublicKey, String>>>,
        events: Option<mpsc::Sender<MediaEvent>>,
        my_path: Option<String>,
        muted: bool,
        devices: DeviceChoice,
        cues: Vec<Cue>,
        /// How often the system's devices were listed: slow on a real machine.
        listings: usize,
    }

    /// Records what the core asks of the media layer, and lets a test report
    /// events as if audio were flowing.
    #[derive(Clone, Default)]
    pub(crate) struct FakeMedia(Arc<Mutex<Live>>);

    impl FakeMedia {
        fn live(&self) -> std::sync::MutexGuard<'_, Live> {
            self.0.lock().expect("fake media lock")
        }

        /// The devices the media layer may exchange audio with right now.
        pub(crate) fn peers(&self) -> BTreeMap<PublicKey, String> {
            self.live()
                .peers
                .as_ref()
                .map(|peers| peers.borrow().clone())
                .unwrap_or_default()
        }

        pub(crate) fn listings(&self) -> usize {
            self.live().listings
        }

        pub(crate) fn my_path(&self) -> Option<String> {
            self.live().my_path.clone()
        }

        pub(crate) fn is_muted(&self) -> bool {
            self.live().muted
        }

        /// The devices the media layer was last told to use.
        pub(crate) fn devices(&self) -> DeviceChoice {
            self.live().devices.clone()
        }

        /// Every sound played so far, oldest first.
        pub(crate) fn cues(&self) -> Vec<Cue> {
            self.live().cues.clone()
        }

        pub(crate) async fn report(&self, event: MediaEvent) {
            let events = self.live().events.clone().expect("no call is running");
            events.send(event).await.expect("the call's events are gone");
        }
    }

    impl HuddleMedia for FakeMedia {
        fn start(&self, call: MediaCall) -> Result<()> {
            let mut live = self.live();
            live.peers = Some(call.peers);
            live.events = Some(call.events);
            live.my_path = Some(call.my_path);
            live.muted = call.muted;
            live.devices = call.devices;
            Ok(())
        }

        fn stop(&self) {
            let mut live = self.live();
            let devices = std::mem::take(&mut live.devices);
            let cues = std::mem::take(&mut live.cues);
            *live = Live {
                devices,
                cues,
                ..Live::default()
            };
        }

        fn set_muted(&self, muted: bool) -> Result<()> {
            self.live().muted = muted;
            Ok(())
        }

        fn set_devices(&self, choice: DeviceChoice) -> Result<()> {
            self.live().devices = choice;
            Ok(())
        }

        fn list_devices(&self) -> Result<DeviceList> {
            self.live().listings += 1;
            let device = |id: &str, is_default: bool, is_bluetooth: bool| AudioDevice {
                id: id.into(),
                name: id.into(),
                is_default,
                is_bluetooth,
            };
            Ok(DeviceList {
                inputs: vec![
                    device("built-in-mic", true, false),
                    device("usb-mic", false, false),
                    device("headset-mic", false, true),
                ],
                outputs: vec![
                    device("built-in-speakers", true, false),
                    device("headset", false, true),
                ],
            })
        }

        fn play_cue(&self, cue: Cue) {
            self.live().cues.push(cue);
        }
    }
}
