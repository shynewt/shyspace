import type { Member, PresenceState } from "./api";

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

/** Whether another active member shows under the same name, so a name alone can't tell them apart. */
function sharesName(member: Member, members: readonly Member[]): boolean {
  const name = displayName(member.name, member.id);
  return members.some((other) => other.id !== member.id && other.active && displayName(other.name, other.id) === name);
}

/** A member's name, with the start of their safety code when another member shares it: "Grace · 1234 5678". */
export function distinctName(member: Member, members: readonly Member[]): string {
  const name = displayName(member.name, member.id);
  return sharesName(member, members) ? `${name} · ${member.safety_code.slice(0, 9)}` : name;
}

/**
 * The word a mention of `member` is written as: their name, or "Grace·12345678"
 * when another member shares it. Those are the digits the app shows, which no
 * two members' codes may share.
 */
export function mentionName(member: Member, members: readonly Member[]): string {
  return sharesName(member, members)
    ? `${member.name}·${member.safety_code.replace(/ /g, "").slice(0, 8)}`
    : member.name;
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
  // Another year's day says its year, so it reads right and isn't taken for this year's same date.
  const year = date.getFullYear() === today.getFullYear() ? undefined : "numeric";
  return date.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric", year });
}

export type DayGroup<Item> = { label: string; items: Item[] };

/**
 * Groups items by day ("Today", "Monday, April 27"), in the order each day
 * first appears. Every item of a day joins its group wherever it falls, so a
 * day never shows twice.
 */
export function groupByDay<Item>(items: Item[], timeOf: (item: Item) => number): DayGroup<Item>[] {
  const days = new Map<string, DayGroup<Item>>();
  for (const item of items) {
    const label = formatDay(timeOf(item));
    const day = days.get(label);
    if (day) day.items.push(item);
    else days.set(label, { label, items: [item] });
  }
  return [...days.values()];
}
