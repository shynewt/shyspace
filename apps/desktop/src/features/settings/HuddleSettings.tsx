import { api } from "../../lib/api";
import { usePreferences } from "../../lib/preferences";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { Notice, Switch } from "../../ui";
import { BluetoothMicHint, DeviceSelect } from "../huddles/HuddleDetails";
import { useHuddleDevices } from "../huddles/useHuddleDevices";
import { Setting } from "./layout";

/**
 * Which microphone and speakers huddles use, whether you join muted, and the
 * join and leave sounds. Without voice in this build, only why.
 */
export function HuddleSettings() {
  const loaded = useHuddleDevices();
  const areSoundsOn = useLive(api.huddleSounds, (change) => change.kind === "huddle_sounds");
  const [preferences, update] = usePreferences();
  if (loaded?.status === "unavailable") {
    return (
      <Notice tone="warning" title="Voice isn't available">
        {loaded.reason}
      </Notice>
    );
  }
  if (!loaded) return null;
  const { devices } = loaded;
  return (
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
      <Setting title="Joining" description="Others hear you only once you unmute.">
        <Switch
          label="Join huddles muted"
          checked={preferences.joinsHuddlesMuted}
          onChange={(joinsHuddlesMuted) => update({ joinsHuddlesMuted })}
        />
      </Setting>
      {areSoundsOn !== undefined && (
        <Setting
          title="Sounds"
          description="A soft tone when you or others join or leave. Only people in the huddle hear it."
        >
          <Switch
            label="Play huddle sounds"
            checked={areSoundsOn}
            onChange={(isOn) => {
              api.setHuddleSounds(isOn).catch(reportError);
            }}
          />
        </Setting>
      )}
    </>
  );
}
