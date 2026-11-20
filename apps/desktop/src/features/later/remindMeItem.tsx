import { AlarmClock, AlarmClockOff, CalendarClock } from "lucide-react";
import { reminderChoices } from "../../lib/reminders";
import type { MenuItem } from "../../ui";

/**
 * The "Remind me" submenu, the same on messages and in Later: presets that
 * fit the moment, "Custom…" for the picker, and a way to drop the reminder
 * once there is one. `onRemind(null)` keeps the item saved, without a time.
 */
export function remindMeItem({
  remindMs,
  onRemind,
  onCustom,
}: {
  remindMs: number | null;
  onRemind: (remindMs: number | null) => void;
  onCustom: () => void;
}): MenuItem {
  const hasReminder = remindMs !== null;
  return {
    label: hasReminder ? "Change reminder" : "Remind me",
    icon: <AlarmClock size={14} />,
    choices: [
      ...reminderChoices(new Date()).map((choice) => ({
        label: choice.label,
        isChosen: false,
        onSelect: () => onRemind(choice.at.getTime()),
      })),
      { label: "Custom…", icon: <CalendarClock size={14} />, isChosen: false, onSelect: onCustom },
      ...(hasReminder
        ? [
            {
              label: "Don't remind me",
              icon: <AlarmClockOff size={14} />,
              isChosen: false,
              onSelect: () => onRemind(null),
            },
          ]
        : []),
    ],
  };
}
