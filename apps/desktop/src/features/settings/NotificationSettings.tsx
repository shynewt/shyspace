import { Undo2 } from "lucide-react";
import { useState } from "react";
import { api, type Channel, type NotifyLevel } from "../../lib/api";
import { useChannelNames } from "../../lib/channels";
import { usePreferences } from "../../lib/preferences";
import { errorMessage } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { Button, FormError, SegmentedControl } from "../../ui";
import { ChannelName } from "../private/ChannelName";
import { AppIconSettings } from "./AppIconSettings";
import { Setting } from "./layout";

const DEFAULT_LEVEL: NotifyLevel = "all";

const LEVELS: Array<{ value: NotifyLevel; label: string }> = [
  { value: "all", label: "Every message" },
  { value: "mentions", label: "Mentions only" },
  { value: "nothing", label: "Muted" },
];

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
      <ChannelAlerts />
      <Setting
        title="Your status"
        description="Your status can make alerts quieter for a while, from the menu under your name. While it's set, the quieter of the status and a channel's own setting wins."
      />
      <AppIconSettings />
    </>
  );
}

type SavedLevel = { channel: Channel; level: NotifyLevel };

function ChannelAlerts() {
  const { channels } = useWorkspace();
  const names = useChannelNames();
  // A toast's Undo can't be reached while Settings is open (it's modal), so it's offered here.
  const [beforeReset, setBeforeReset] = useState<SavedLevel[] | null>(null);
  // Toasts sit behind the modal Settings window, so failures show here too.
  const [error, setError] = useState<string | null>(null);
  // Your notes never alert you: you wrote them.
  const sorted = channels
    .filter((channel) => channel.kind !== "notes")
    .sort((left, right) => left.name.localeCompare(right.name));
  const changed = sorted.filter((channel) => channel.notify !== DEFAULT_LEVEL);

  /** Sets each level, reporting the ones that failed; resolves to those that worked. */
  const setLevels = async (levels: SavedLevel[]): Promise<SavedLevel[]> => {
    const results = await Promise.allSettled(levels.map(({ channel, level }) => api.setNotifyLevel(channel.id, level)));
    const failures = results.flatMap((result, index) =>
      result.status === "rejected"
        ? [`${names.reference(levels[index].channel.id)}: ${errorMessage(result.reason)}`]
        : [],
    );
    setError(failures.length > 0 ? `Couldn't change ${failures.join("; ")}` : null);
    return levels.filter((_, index) => results[index].status === "fulfilled");
  };
  const resetAll = async () => {
    const previous = changed.map((channel) => ({ channel, level: channel.notify }));
    const reset = await setLevels(previous.map(({ channel }) => ({ channel, level: DEFAULT_LEVEL })));
    const resetChannels = new Set(reset.map(({ channel }) => channel.id));
    const undoable = previous.filter(({ channel }) => resetChannels.has(channel.id));
    setBeforeReset(undoable.length > 0 ? undoable : null);
  };
  const undoReset = async (previous: SavedLevel[]) => {
    const restored = await setLevels(previous);
    const restoredChannels = new Set(restored.map(({ channel }) => channel.id));
    const stillReset = previous.filter(({ channel }) => !restoredChannels.has(channel.id));
    setBeforeReset(stillReset.length > 0 ? stillReset : null);
  };
  return (
    <Setting
      title="Channels"
      description="Which messages alert you, per channel. A channel's menu in the sidebar changes this too."
    >
      <ul aria-label="Channel alerts" className="flex flex-col">
        {sorted.map((channel) => (
          <li key={channel.id} className="flex items-center gap-3 py-1">
            <ChannelName channelId={channel.id} className="flex-1 text-sm" />
            <SegmentedControl<NotifyLevel>
              label={`Alerts for ${names.reference(channel.id)}`}
              options={LEVELS}
              value={channel.notify}
              onChange={(level) => setLevels([{ channel, level }])}
            />
          </li>
        ))}
      </ul>
      <div className="flex items-center gap-3">
        <Button size="sm" disabled={changed.length === 0} onClick={resetAll}>
          Reset all to every message
        </Button>
        {beforeReset && (
          <p role="status" className="flex items-center gap-1 text-sm text-muted">
            {beforeReset.length === 1 ? "1 channel was reset." : `${beforeReset.length} channels were reset.`}
            <Button variant="ghost" size="sm" icon={<Undo2 size={13} />} onClick={() => undoReset(beforeReset)}>
              Undo
            </Button>
          </p>
        )}
      </div>
      <FormError message={error} />
    </Setting>
  );
}
