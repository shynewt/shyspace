//! Which microphone and speaker a call really opens, from the person's
//! choice and what the system offers right now.

use shy_core::huddle::media::{AudioDevice, DeviceChoice, DeviceList, OutputState};

/// One device to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    /// `None` opens the system's default.
    pub(crate) device: Option<String>,
    /// The chosen device is gone, so the default is used instead.
    pub(crate) is_lost: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDevices {
    pub(crate) input: Resolved,
    pub(crate) output: Resolved,
}

/// A chosen device that's still there is used as is. Otherwise the default
/// is, except that with a Bluetooth headset as the speaker, a microphone
/// that isn't Bluetooth is preferred: using the headset's own microphone
/// drops it to call quality.
pub(crate) fn resolve(choice: &DeviceChoice, offered: &DeviceList) -> ResolvedDevices {
    let output = resolve_one(choice.output.as_deref(), &offered.outputs);
    let mut input = resolve_one(choice.input.as_deref(), &offered.inputs);
    if input.device.is_none() && is_bluetooth(in_use(&output, &offered.outputs)) {
        input.device = microphone_beside_a_headset(&offered.inputs);
    }
    ResolvedDevices { input, output }
}

/// The choice as it is, for when the system won't list its devices: a
/// chosen device is tried rather than taken for gone.
pub(crate) fn unchecked(choice: &DeviceChoice) -> ResolvedDevices {
    let tried = |device: &Option<String>| Resolved {
        device: device.clone(),
        is_lost: false,
    };
    ResolvedDevices {
        input: tried(&choice.input),
        output: tried(&choice.output),
    }
}

/// What a working speaker is doing. A Bluetooth headset whose own
/// microphone is open drops to its call mode (mono, low quality); the
/// system doesn't say so, so it's worked out from which devices are in use.
pub(crate) fn output_state(
    resolved: &ResolvedDevices,
    offered: &DeviceList,
    is_mic_open: bool,
) -> OutputState {
    let headset_both_ways = is_bluetooth(in_use(&resolved.output, &offered.outputs))
        && is_bluetooth(in_use(&resolved.input, &offered.inputs));
    if is_mic_open && headset_both_ways {
        OutputState::BluetoothCallMode
    } else {
        OutputState::Playing
    }
}

/// A device as the system lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) is_default: bool,
    /// The sound system it's reached through, such as "PipeWire" or "ALSA".
    pub(crate) host: String,
}

/// The devices to offer: those of the sound system the default comes from.
/// The same hardware is listed once per sound system, and offering each
/// copy would show every microphone twice.
pub(crate) fn offer(listed: impl IntoIterator<Item = Listed>) -> Vec<AudioDevice> {
    let listed: Vec<Listed> = listed.into_iter().collect();
    let default_host = listed
        .iter()
        .find(|device| device.is_default)
        .map(|device| device.host.clone());
    listed
        .into_iter()
        .filter(|device| default_host.as_ref().is_none_or(|host| device.host == *host))
        .filter(|device| !is_stand_in_for_the_default(&device.id))
        .map(|device| AudioDevice {
            is_bluetooth: is_bluetooth_id(&device.id),
            id: device.id,
            name: device.name,
            is_default: device.is_default,
        })
        .collect()
}

/// PipeWire lists the system default as devices of its own, which follow
/// whatever the default is. Choosing no device does the same, and the
/// pickers offer that as the system default already.
fn is_stand_in_for_the_default(id: &str) -> bool {
    matches!(
        id,
        "pipewire:input_default" | "pipewire:output_default" | "pipewire:sink_default"
    )
}

/// The inputs that are microphones. A Linux sound server also lists every
/// speaker as something to record from, which captures all the sound the
/// computer plays; offered as a microphone, it would send the call
/// everything from other calls to notifications. Elsewhere a device that
/// both plays and records, like a USB headset, is one real device and stays.
pub(crate) fn microphones(inputs: Vec<Listed>, outputs: &[Listed]) -> Vec<Listed> {
    inputs
        .into_iter()
        .filter(|input| {
            let is_sound_server = matches!(host_of(&input.id), "pipewire" | "pulseaudio");
            !(is_sound_server && outputs.iter().any(|output| output.id == input.id))
        })
        .collect()
}

fn host_of(id: &str) -> &str {
    id.split_once(':').map_or("", |(host, _device)| host)
}

/// Whether a system device id (cpal's `host:device`) names a Bluetooth
/// device. Linux sound servers name them `bluez_…` and BlueALSA `bluealsa…`;
/// macOS uses a Bluetooth device's address as its UID.
pub(crate) fn is_bluetooth_id(id: &str) -> bool {
    let device = id.split_once(':').map_or(id, |(_host, device)| device);
    device.starts_with("bluez_") || device.starts_with("bluealsa") || starts_with_address(device)
}

/// Six pairs of hex digits joined by `-` or `:`, as `48-B8-A3-12-34-56`.
fn starts_with_address(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 17 {
        return false;
    }
    bytes[..17]
        .iter()
        .enumerate()
        .all(|(index, byte)| match index % 3 {
            2 => matches!(byte, b'-' | b':'),
            _ => byte.is_ascii_hexdigit(),
        })
}

fn resolve_one(chosen: Option<&str>, offered: &[AudioDevice]) -> Resolved {
    let Some(chosen) = chosen else {
        return Resolved {
            device: None,
            is_lost: false,
        };
    };
    let is_there = offered.iter().any(|device| device.id == chosen);
    Resolved {
        device: is_there.then(|| chosen.to_owned()),
        is_lost: !is_there,
    }
}

/// The device `resolved` opens, as far as the system tells.
fn in_use<'a>(resolved: &Resolved, offered: &'a [AudioDevice]) -> Option<&'a AudioDevice> {
    match &resolved.device {
        Some(id) => offered.iter().find(|device| device.id == *id),
        None => offered.iter().find(|device| device.is_default),
    }
}

fn is_bluetooth(device: Option<&AudioDevice>) -> bool {
    device.is_some_and(|device| device.is_bluetooth)
}

/// `None` keeps the default: it isn't Bluetooth, or nothing else is there.
fn microphone_beside_a_headset(inputs: &[AudioDevice]) -> Option<String> {
    let default = inputs.iter().find(|device| device.is_default);
    if default.is_some_and(|device| !device.is_bluetooth) {
        return None;
    }
    inputs
        .iter()
        .find(|device| !device.is_bluetooth)
        .map(|device| device.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, is_default: bool, is_bluetooth: bool) -> AudioDevice {
        AudioDevice {
            id: id.into(),
            name: id.into(),
            is_default,
            is_bluetooth,
        }
    }

    /// A laptop with a Bluetooth headset connected, the headset as default.
    fn headset_as_default() -> DeviceList {
        DeviceList {
            inputs: vec![
                device("headset-mic", true, true),
                device("built-in-mic", false, false),
            ],
            outputs: vec![
                device("headset", true, true),
                device("built-in-speakers", false, false),
            ],
        }
    }

    fn choice(input: Option<&str>, output: Option<&str>) -> DeviceChoice {
        DeviceChoice {
            input: input.map(Into::into),
            output: output.map(Into::into),
        }
    }

    fn opens(device: Option<&str>) -> Resolved {
        Resolved {
            device: device.map(Into::into),
            is_lost: false,
        }
    }

    #[test]
    fn bluetooth_devices_are_told_by_their_ids() {
        for id in [
            "pipewire:bluez_output.48_B8_A3_12_34_56.1",
            "pipewire:bluez_input.48_B8_A3_12_34_56.0",
            "pulseaudio:bluez_sink.48_B8_A3_12_34_56.a2dp_sink",
            "alsa:bluealsa",
            "coreaudio:48-B8-A3-12-34-56:output",
        ] {
            assert!(is_bluetooth_id(id), "{id}");
        }
        for id in [
            "pipewire:alsa_output.pci-0000_00_1f.3.analog-stereo",
            "alsa:hw:0,0",
            "coreaudio:BuiltInSpeakerDevice",
            "coreaudio:AppleUSBAudioEngine:Generic:USB Audio:2100000:1",
            "coreaudio:48-B8",
        ] {
            assert!(!is_bluetooth_id(id), "{id}");
        }
    }

    fn listed(id: &str, host: &str, is_default: bool) -> Listed {
        Listed {
            id: id.into(),
            name: id.into(),
            is_default,
            host: host.into(),
        }
    }

    #[test]
    fn only_the_default_sound_systems_devices_are_offered() {
        let offered = offer([
            listed("alsa:hw:0,0", "ALSA", false),
            listed("pipewire:alsa_output.analog-stereo", "PipeWire", true),
            listed("pipewire:bluez_output.48_B8_A3_12_34_56.1", "PipeWire", false),
        ]);
        let ids: Vec<&str> = offered.iter().map(|device| device.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "pipewire:alsa_output.analog-stereo",
                "pipewire:bluez_output.48_B8_A3_12_34_56.1"
            ]
        );
        assert!(offered[0].is_default && !offered[0].is_bluetooth);
        assert!(!offered[1].is_default && offered[1].is_bluetooth);
    }

    #[test]
    fn pipewires_stand_ins_for_the_default_arent_offered() {
        let offered = offer([
            listed("alsa:hw:0,0", "ALSA", false),
            listed("pipewire:sink_default", "PipeWire", false),
            listed("pipewire:output_default", "PipeWire", true),
            listed("pipewire:alsa_output.analog-stereo", "PipeWire", false),
        ]);
        let ids: Vec<&str> = offered.iter().map(|device| device.id.as_str()).collect();
        assert_eq!(
            ids,
            ["pipewire:alsa_output.analog-stereo"],
            "the real ones, of the default's host only"
        );
    }

    #[test]
    fn without_a_default_every_device_is_offered() {
        let offered = offer([
            listed("alsa:hw:0,0", "ALSA", false),
            listed("jack:system", "JACK", false),
        ]);
        assert_eq!(offered.len(), 2);
    }

    #[test]
    fn a_sound_servers_speakers_arent_offered_as_microphones() {
        let outputs = [
            listed("pipewire:sink_default", "PipeWire", false),
            listed("pipewire:alsa_output.analog-stereo", "PipeWire", false),
        ];
        let inputs = vec![
            listed("pipewire:sink_default", "PipeWire", false),
            listed("pipewire:input_default", "PipeWire", true),
            listed("pipewire:alsa_output.analog-stereo", "PipeWire", false),
            listed("pipewire:alsa_input.analog-stereo", "PipeWire", false),
        ];
        let ids: Vec<String> = microphones(inputs, &outputs)
            .into_iter()
            .map(|device| device.id)
            .collect();
        assert_eq!(
            ids,
            ["pipewire:input_default", "pipewire:alsa_input.analog-stereo"]
        );
    }

    #[test]
    fn a_headset_that_plays_and_records_stays_a_microphone_elsewhere() {
        let headset = listed("coreaudio:AppleUSBAudioEngine:Headset:1", "CoreAudio", false);
        let kept = microphones(vec![headset.clone()], std::slice::from_ref(&headset));
        assert_eq!(kept, [headset]);
    }

    #[test]
    fn with_no_choice_the_defaults_open() {
        let offered = DeviceList {
            inputs: vec![device("built-in-mic", true, false)],
            outputs: vec![device("built-in-speakers", true, false)],
        };
        let resolved = resolve(&DeviceChoice::default(), &offered);
        assert_eq!(resolved.input, opens(None));
        assert_eq!(resolved.output, opens(None));
    }

    #[test]
    fn a_chosen_device_that_is_there_opens() {
        let resolved = resolve(
            &choice(Some("headset-mic"), Some("built-in-speakers")),
            &headset_as_default(),
        );
        assert_eq!(resolved.input, opens(Some("headset-mic")));
        assert_eq!(resolved.output, opens(Some("built-in-speakers")));
    }

    #[test]
    fn a_chosen_device_that_is_gone_falls_back_to_the_default_and_says_so() {
        let offered = DeviceList {
            inputs: vec![device("built-in-mic", true, false)],
            outputs: vec![device("built-in-speakers", true, false)],
        };
        let resolved = resolve(&choice(Some("usb-mic"), Some("usb-speakers")), &offered);
        let lost = Resolved {
            device: None,
            is_lost: true,
        };
        assert_eq!(resolved.input, lost);
        assert_eq!(resolved.output, lost);
    }

    #[test]
    fn a_headset_as_speaker_keeps_the_built_in_microphone() {
        let resolved = resolve(&DeviceChoice::default(), &headset_as_default());
        assert_eq!(resolved.input, opens(Some("built-in-mic")));
    }

    #[test]
    fn a_headset_chosen_as_speaker_keeps_the_built_in_microphone() {
        let mut offered = headset_as_default();
        for output in &mut offered.outputs {
            output.is_default = !output.is_bluetooth;
        }
        let resolved = resolve(&choice(None, Some("headset")), &offered);
        assert_eq!(resolved.input, opens(Some("built-in-mic")));
    }

    #[test]
    fn a_lost_microphone_beside_a_headset_falls_back_to_the_built_in_one() {
        let resolved = resolve(&choice(Some("usb-mic"), None), &headset_as_default());
        assert_eq!(
            resolved.input,
            Resolved {
                device: Some("built-in-mic".into()),
                is_lost: true,
            }
        );
    }

    #[test]
    fn a_chosen_headset_microphone_is_respected() {
        let resolved = resolve(&choice(Some("headset-mic"), None), &headset_as_default());
        assert_eq!(resolved.input, opens(Some("headset-mic")));
    }

    #[test]
    fn a_default_microphone_that_isnt_bluetooth_stays_the_default() {
        let mut offered = headset_as_default();
        offered.inputs = vec![
            device("headset-mic", false, true),
            device("usb-mic", true, false),
            device("built-in-mic", false, false),
        ];
        let resolved = resolve(&DeviceChoice::default(), &offered);
        assert_eq!(resolved.input, opens(None));
    }

    #[test]
    fn a_headset_alone_uses_its_own_microphone() {
        let offered = DeviceList {
            inputs: vec![device("headset-mic", true, true)],
            outputs: vec![device("headset", true, true)],
        };
        let resolved = resolve(&DeviceChoice::default(), &offered);
        assert_eq!(resolved.input, opens(None));
    }

    #[test]
    fn speakers_that_arent_bluetooth_leave_the_microphone_alone() {
        let mut offered = headset_as_default();
        for output in &mut offered.outputs {
            output.is_default = !output.is_bluetooth;
        }
        let resolved = resolve(&DeviceChoice::default(), &offered);
        assert_eq!(resolved.input, opens(None));
    }

    #[test]
    fn a_headset_used_both_ways_is_in_call_mode_only_while_the_microphone_is_open() {
        let resolved = resolve(&choice(Some("headset-mic"), None), &headset_as_default());
        assert_eq!(
            output_state(&resolved, &headset_as_default(), true),
            OutputState::BluetoothCallMode
        );
        assert_eq!(
            output_state(&resolved, &headset_as_default(), false),
            OutputState::Playing
        );
    }

    #[test]
    fn a_headset_beside_the_built_in_microphone_plays_normally() {
        let resolved = resolve(&DeviceChoice::default(), &headset_as_default());
        assert_eq!(
            output_state(&resolved, &headset_as_default(), true),
            OutputState::Playing
        );
    }

    #[test]
    fn a_gone_speaker_replaced_by_the_default_plays() {
        let resolved = resolve(&choice(None, Some("usb-speakers")), &headset_as_default());
        assert_eq!(
            output_state(&resolved, &headset_as_default(), false),
            OutputState::Playing
        );
    }
}
