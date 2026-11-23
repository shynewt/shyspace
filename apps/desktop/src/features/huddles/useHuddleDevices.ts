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

/** The device in use: the chosen one, or the one the system marks as its default (some mark none). */
export function deviceInUse(devices: AudioDevice[], chosen: string | null): AudioDevice | undefined {
  return chosen === null ? devices.find((device) => device.is_default) : devices.find((device) => device.id === chosen);
}

/**
 * Stands for choosing no device, which follows the system's default wherever
 * it moves. Device ids are the sound system's ("pipewire:…", "alsa:…"), so
 * this can't be one; and a picker's values can't be empty.
 */
const SYSTEM_DEFAULT = "shyspace:system-default";

export type DeviceChoice = { value: string; label: string; device?: AudioDevice; isChosen: boolean };

/**
 * What a microphone (`input`) or speakers (`output`) picker offers: the
 * system's default, then each device. A chosen device that's unplugged stays
 * chosen, since the core goes back to it when it returns.
 */
export function deviceChoices(devices: AudioDevices, kind: "input" | "output"): DeviceChoice[] {
  const list = kind === "input" ? devices.inputs : devices.outputs;
  const chosen = devices.chosen[kind];
  const systemDefault = list.find((device) => device.is_default);
  const choices: DeviceChoice[] = [
    {
      value: SYSTEM_DEFAULT,
      label: systemDefault ? `System default (${systemDefault.name})` : "System default",
      isChosen: chosen === null,
    },
  ];
  if (chosen !== null && !list.some((device) => device.id === chosen)) {
    choices.push({ value: chosen, label: "Not connected, using the system default", isChosen: true });
  }
  for (const device of list) {
    choices.push({ value: device.id, label: device.name, device, isChosen: device.id === chosen });
  }
  return choices;
}

/** Chooses a picker's `value` for its kind. */
export function chooseDevice(devices: AudioDevices, kind: "input" | "output", value: string) {
  const choice = { ...devices.chosen, [kind]: value === SYSTEM_DEFAULT ? null : value };
  api.setHuddleDevices(choice).catch(reportError);
}

/**
 * A microphone that won't drop a Bluetooth headset to call quality, as a
 * picker value: the system's default if it's fine, else the first that is.
 */
export function fullQualityMic(devices: AudioDevices): { name: string; value: string } | undefined {
  const systemDefault = devices.inputs.find((device) => device.is_default);
  if (systemDefault && !systemDefault.is_bluetooth) return { name: systemDefault.name, value: SYSTEM_DEFAULT };
  const fine = devices.inputs.find((device) => !device.is_bluetooth);
  return fine && { name: fine.name, value: fine.id };
}
