import type { PresenceState } from "./api";

const PRESENCE_LABELS: Record<PresenceState, string> = {
  online: "Online",
  away: "Away",
  unreachable: "Not reachable right now",
};

export function presenceLabel(presence: PresenceState): string {
  return PRESENCE_LABELS[presence];
}

export function displayName(name: string, id: string) {
  return name.trim() || `member ${id.slice(0, 6)}`;
}

export function formatTime(ms: number) {
  return new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

/** When a status ends: "17:00" later today, "Sat 02:00" on another day. */
export function formatStatusEnd(ms: number): string {
  const isToday = new Date(ms).toDateString() === new Date().toDateString();
  return isToday ? formatTime(ms) : `${new Date(ms).toLocaleDateString([], { weekday: "short" })} ${formatTime(ms)}`;
}

/** "Friday, November 20, 2026 at 9:05 AM". */
export function formatFullDate(ms: number) {
  return new Date(ms).toLocaleString([], { dateStyle: "full", timeStyle: "short" });
}

/** A past moment, as short as stays clear: "4:41 PM", "Yesterday 4:41 PM", "Mon 4:41 PM", "Nov 3", "Nov 3, 2025". */
export function formatWhen(ms: number, now: Date) {
  const date = new Date(ms);
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  const sixDaysAgo = new Date(now);
  sixDaysAgo.setDate(now.getDate() - 6);
  sixDaysAgo.setHours(0, 0, 0, 0);
  if (date.toDateString() === now.toDateString()) return formatTime(ms);
  if (date.toDateString() === yesterday.toDateString()) return `Yesterday ${formatTime(ms)}`;
  if (date >= sixDaysAgo) return `${date.toLocaleDateString([], { weekday: "short" })} ${formatTime(ms)}`;
  if (date.getFullYear() === now.getFullYear()) return date.toLocaleDateString([], { month: "short", day: "numeric" });
  return date.toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" });
}

export function formatDay(ms: number) {
  const date = new Date(ms);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (date.toDateString() === today.toDateString()) return "Today";
  if (date.toDateString() === yesterday.toDateString()) return "Yesterday";
  return date.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric" });
}

export type DayGroup<Item> = { label: string; items: Item[] };

/** Splits items already in time order into runs that share a day ("Today", "Monday, April 27"). */
export function groupByDay<Item>(items: Item[], timeOf: (item: Item) => number): DayGroup<Item>[] {
  const days: DayGroup<Item>[] = [];
  for (const item of items) {
    const label = formatDay(timeOf(item));
    const last = days.at(-1);
    if (last?.label === label) last.items.push(item);
    else days.push({ label, items: [item] });
  }
  return days;
}
