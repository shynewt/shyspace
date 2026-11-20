/** Reminder presets, in the user's local time. */

import { formatTime } from "./people";

const MINUTE_MS = 60 * 1000;
const MORNING_HOUR = 9;

export type ReminderChoice = { label: string; at: (now: Date) => Date };

function atMorning(date: Date): Date {
  const morning = new Date(date);
  morning.setHours(MORNING_HOUR, 0, 0, 0);
  return morning;
}

export const REMINDER_CHOICES: ReminderChoice[] = [
  { label: "In 20 minutes", at: (now) => new Date(now.getTime() + 20 * MINUTE_MS) },
  { label: "In 1 hour", at: (now) => new Date(now.getTime() + 60 * MINUTE_MS) },
  {
    label: "Tomorrow at 9",
    at: (now) => {
      const tomorrow = new Date(now);
      tomorrow.setDate(now.getDate() + 1);
      return atMorning(tomorrow);
    },
  },
  {
    label: "Next Monday at 9",
    at: (now) => {
      const monday = new Date(now);
      const daysAhead = (8 - now.getDay()) % 7 || 7;
      monday.setDate(now.getDate() + daysAhead);
      return atMorning(monday);
    },
  },
];

/** "Overdue", "Today 3:00 PM", "Tomorrow 9:00 AM", or "Mon, Nov 24 9:00 AM". */
export function describeReminder(remindMs: number, now: Date): string {
  if (remindMs <= now.getTime()) return "Overdue";
  const at = new Date(remindMs);
  const time = formatTime(remindMs);
  const tomorrow = new Date(now);
  tomorrow.setDate(now.getDate() + 1);
  if (at.toDateString() === now.toDateString()) return `Today ${time}`;
  if (at.toDateString() === tomorrow.toDateString()) return `Tomorrow ${time}`;
  return `${at.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" })} ${time}`;
}
