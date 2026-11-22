import { type AudioDevice, type AudioDevices, api } from "../../lib/api";
import { errorMessage, reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";

export type LoadedDevices = { status: "shown"; devices: AudioDevices } | { status: "unavailable"; reason: string };

/** A build without audio can't list devices; that's a state to show, not an error to toast. */
function loadDevices(): Promise<LoadedDevices> {
  return api.huddleDevices().then(
    (devices) => ({ status: "shown", devices }),
    (error: unknown) => ({ status: "unavailable", reason: errorMessage(error) }),
  );
}

/** Your microphones and speakers, and which are chosen, kept current. */
export function useHuddleDevices(): LoadedDevices | undefined {
  return useLive(loadDevices, (change) => change.kind === "huddle_devices");
}

/** The device in use: the chosen one, or the system's default when none is. */
export function deviceInUse(devices: AudioDevice[], chosen: string | null): AudioDevice | undefined {
  return devices.find((device) => device.id === chosen) ?? devices.find((device) => device.is_default);
}

/** Chooses `device` for its kind; choosing the system's default follows it from then on. */
export function chooseDevice(devices: AudioDevices, kind: "input" | "output", device: AudioDevice) {
  const choice = { ...devices.chosen, [kind]: device.is_default ? null : device.id };
  api.setHuddleDevices(choice).catch(reportError);
}

/** A microphone that won't drop a Bluetooth headset to call quality: the default if it's fine, else the first that is. */
export function fullQualityMic(devices: AudioDevices): AudioDevice | undefined {
  return (
    devices.inputs.find((device) => device.is_default && !device.is_bluetooth) ??
    devices.inputs.find((device) => !device.is_bluetooth)
  );
}
