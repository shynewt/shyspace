import { AlarmClock, Bookmark } from "lucide-react";
import type { SavedMark } from "../../lib/api";
import { describeReminder } from "../../lib/reminders";
import { useWorkspace } from "../../lib/workspace";
import { cx, Tooltip } from "../../ui";

/** "Saved for later" or "Reminder · Today 3:00 PM", in Later's words; nothing once done. */
export function describeSaved(saved: SavedMark | null): string | null {
  if (!saved || saved.is_done) return null;
  if (saved.remind_ms === null) return "Saved for later";
  return `Reminder · ${describeReminder(saved.remind_ms, new Date())}`;
}

/** The line above a saved message's text, the same on every message; opens Later. */
export function SavedLabel({ saved }: { saved: SavedMark | null }) {
  const { navigate } = useWorkspace();
  const description = describeSaved(saved);
  if (!saved || !description) return null;
  const remindMs = saved.remind_ms;
  const isDue = remindMs !== null && remindMs <= Date.now();
  return (
    <Tooltip label="Open Later">
      <button
        type="button"
        onClick={() => navigate({ kind: "later" })}
        className={cx(
          "mb-0.5 flex w-fit items-center gap-1 rounded text-[12px] font-medium hover:underline",
          isDue ? "text-danger-text" : "text-accent-text",
        )}
      >
        {remindMs === null ? <Bookmark size={12} /> : <AlarmClock size={12} />}
        {description}
      </button>
    </Tooltip>
  );
}
