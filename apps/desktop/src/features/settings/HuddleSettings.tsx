import { usePreferences } from "../../lib/preferences";
import { Notice, Switch } from "../../ui";
import { BluetoothMicHint, DeviceSelect } from "../huddles/HuddleDetails";
import { useHuddleDevices } from "../huddles/useHuddleDevices";
import { Setting } from "./layout";

/** Which microphone and speakers huddles use, and whether you join muted. */
export function HuddleSettings() {
  const loaded = useHuddleDevices();
  const [preferences, update] = usePreferences();
  const devices = loaded?.status === "shown" ? loaded.devices : null;
  return (
    <>
      {loaded?.status === "unavailable" && (
        <Notice tone="warning" title="Voice isn't available" className="mb-4">
          {loaded.reason}
        </Notice>
      )}
      {devices && (
        <>
          <Setting
            title="Microphone"
            description="With a Bluetooth headset, huddles use your computer's microphone, so what you hear keeps full quality."
          >
            <DeviceSelect devices={devices} kind="input" className="w-80 max-w-full" />
            <BluetoothMicHint devices={devices} />
          </Setting>
          <Setting title="Speakers">
            <DeviceSelect devices={devices} kind="output" className="w-80 max-w-full" />
          </Setting>
        </>
      )}
      <Setting title="Joining" description="Others hear you only once you unmute.">
        <Switch
          label="Join huddles muted"
          checked={preferences.joinsHuddlesMuted}
          onChange={(joinsHuddlesMuted) => update({ joinsHuddlesMuted })}
        />
      </Setting>
    </>
  );
}
