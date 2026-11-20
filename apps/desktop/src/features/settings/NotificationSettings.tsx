import { usePreferences } from "../../lib/preferences";
import { SegmentedControl } from "../../ui";
import { Setting } from "./layout";

export function NotificationSettings() {
  const [preferences, update] = usePreferences();
  return (
    <>
      <Setting
        title="Message text in notifications"
        description="Notifications appear outside shyspace's encryption, in your system's notification center. Hide the text to show only who wrote and where."
      >
        <SegmentedControl<"show" | "hide">
          label="Message text in notifications"
          options={[
            { value: "show", label: "Show text" },
            { value: "hide", label: "Hide text" },
          ]}
          value={preferences.alertsShowText ? "show" : "hide"}
          onChange={(choice) => update({ alertsShowText: choice === "show" })}
        />
      </Setting>
      <p className="text-[12.5px] leading-relaxed text-muted">
        Choose which messages notify you per channel, from a channel's menu: every message, mentions only, or nothing.
      </p>
    </>
  );
}
