import { type ReduceMotion, type Theme, usePreferences } from "../../lib/preferences";
import { SegmentedControl } from "../../ui";
import { Setting } from "./layout";

export function AppearanceSettings() {
  const [preferences, update] = usePreferences();
  return (
    <>
      <Setting title="Theme" description="System follows your computer's light or dark setting.">
        <SegmentedControl<Theme>
          label="Theme"
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
          value={preferences.theme}
          onChange={(theme) => update({ theme })}
        />
      </Setting>
      <Setting
        title="Reduce motion"
        description="Fewer moving and scaling animations. System follows your computer's accessibility setting."
      >
        <SegmentedControl<ReduceMotion>
          label="Reduce motion"
          options={[
            { value: "system", label: "System" },
            { value: "on", label: "On" },
            { value: "off", label: "Off" },
          ]}
          value={preferences.reduceMotion}
          onChange={(reduceMotion) => update({ reduceMotion })}
        />
      </Setting>
    </>
  );
}
