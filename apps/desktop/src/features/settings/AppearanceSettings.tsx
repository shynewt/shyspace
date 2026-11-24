import { PALETTE_NAMES, PALETTE_SWATCHES } from "../../lib/palettes";
import {
  PALETTES,
  type ReduceMotion,
  shownLook,
  type Theme,
  usePreferences,
  WORKSPACE_DEFAULT,
  workspaceDefaults,
} from "../../lib/preferences";
import { SegmentedControl, ThemeSwatches } from "../../ui";
import { useWorkspaces } from "../shell/Workspaces";
import { AccentSwatches } from "./AccentSwatches";
import { Setting } from "./layout";

export function AppearanceSettings() {
  const [preferences, update] = usePreferences();
  const { look } = useWorkspaces();
  const defaults = workspaceDefaults(look);
  const shown = shownLook(preferences, look);
  return (
    <>
      <Setting
        title="Theme"
        description="Each theme has a light and a dark side. Picking one changes only this device; picking the workspace's follows it."
      >
        <ThemeSwatches
          label="Theme"
          options={PALETTES.map((palette) => ({
            value: palette,
            label: PALETTE_NAMES[palette],
            detail: palette === defaults.palette ? "the workspace's default" : undefined,
            ...PALETTE_SWATCHES[palette],
          }))}
          value={shown.palette}
          // Picking the workspace's own follows it from then on.
          onChange={(palette) => update({ palette: palette === defaults.palette ? WORKSPACE_DEFAULT : palette })}
        />
      </Setting>
      <Setting title="Light or dark" description="System follows your computer's light or dark setting.">
        <SegmentedControl<Theme>
          label="Light or dark"
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
          value={preferences.theme}
          onChange={(theme) => update({ theme })}
        />
      </Setting>
      <Setting title="Accent colour" description="Buttons, links, highlights and the focus ring.">
        <AccentSwatches
          value={shown.accent}
          onChange={(accent) => update({ accent: accent === defaults.accent ? WORKSPACE_DEFAULT : accent })}
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
      <Setting
        title="Others' cursors in pages"
        description="Where others on the same page are typing and what they've selected, in the colour of their avatar."
      >
        <SegmentedControl<"show" | "hide">
          label="Others' cursors in pages"
          options={[
            { value: "show", label: "Show" },
            { value: "hide", label: "Hide" },
          ]}
          value={preferences.showCursorsInPages ? "show" : "hide"}
          onChange={(choice) => update({ showCursorsInPages: choice === "show" })}
        />
      </Setting>
    </>
  );
}
