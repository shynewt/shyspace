/** Reminder times, in the user's local time: presets that fit the moment, and typed times. */

import { formatTime } from "./people";

const MINUTE_MS = 60 * 1000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const MORNING_HOUR = 9;
/** "Later today" is offered only if it still lands before evening. */
const EVENING_HOUR = 20;
const SATURDAY = 6;

export type ReminderChoice = { label: string; at: Date };

function atHour(date: Date, hour: number, minute = 0): Date {
  const at = new Date(date);
  at.setHours(hour, minute, 0, 0);
  return at;
}

function daysAfter(date: Date, days: number): Date {
  const later = new Date(date);
  later.setDate(date.getDate() + days);
  return later;
}

/** The next `weekday` (0 is Sunday) after `date`, never `date` itself. */
function nextWeekday(date: Date, weekday: number): Date {
  return daysAfter(date, (weekday - date.getDay() + 7) % 7 || 7);
}

/** The presets that make sense at `now`, like Gmail's: no "later today" in the evening, no "this weekend" at the weekend. */
export function reminderChoices(now: Date): ReminderChoice[] {
  const choices: ReminderChoice[] = [
    { label: "In 20 minutes", at: new Date(now.getTime() + 20 * MINUTE_MS) },
    { label: "In 1 hour", at: new Date(now.getTime() + HOUR_MS) },
  ];
  const laterToday = new Date(now.getTime() + 3 * HOUR_MS);
  if (laterToday.toDateString() === now.toDateString() && laterToday.getHours() < EVENING_HOUR) {
    choices.push({ label: `Later today · ${formatTime(laterToday.getTime())}`, at: laterToday });
  }
  const tomorrow = atHour(daysAfter(now, 1), MORNING_HOUR);
  choices.push({ label: `Tomorrow · ${formatTime(tomorrow.getTime())}`, at: tomorrow });
  const isMondayToThursday = now.getDay() >= 1 && now.getDay() <= 4;
  if (isMondayToThursday) {
    const saturday = atHour(nextWeekday(now, SATURDAY), MORNING_HOUR);
    choices.push({ label: `This weekend · Sat ${formatTime(saturday.getTime())}`, at: saturday });
  }
  const monday = atHour(nextWeekday(now, 1), MORNING_HOUR);
  choices.push({ label: `Next week · Mon ${formatTime(monday.getTime())}`, at: monday });
  return choices;
}

/** The next whole hour after `now`: where a custom reminder starts. */
export function nextRoundHour(now: Date): Date {
  const next = atHour(now, now.getHours());
  next.setHours(next.getHours() + 1);
  return next;
}

const WEEKDAYS = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
const UNITS_MS: [RegExp, number][] = [
  [/^(m|min|mins|minutes?)$/, MINUTE_MS],
  [/^(h|hr|hrs|hours?)$/, HOUR_MS],
  [/^(d|days?)$/, DAY_MS],
  [/^(w|wk|weeks?)$/, 7 * DAY_MS],
];

/** "in 2h", "in 30 min", "in 3 days". */
function parseDelay(text: string, now: Date): Date | null {
  const match = /^in\s+(\d{1,3})\s*([a-z]+)$/.exec(text);
  if (!match) return null;
  const unit = UNITS_MS.find(([pattern]) => pattern.test(match[2]));
  return unit ? new Date(now.getTime() + Number(match[1]) * unit[1]) : null;
}

/** "9", "9am", "9:30 pm", "15:00", "noon": hours and minutes, or null. */
export function parseTimeOfDay(typed: string): { hour: number; minute: number } | null {
  const text = typed.trim().toLowerCase();
  if (text === "noon") return { hour: 12, minute: 0 };
  const match = /^(\d{1,2})(?::(\d{2}))?\s*(am|pm)?$/.exec(text);
  if (!match) return null;
  let hour = Number(match[1]);
  const minute = match[2] ? Number(match[2]) : 0;
  const half = match[3];
  if (minute > 59 || hour > 23 || (half && (hour === 0 || hour > 12))) return null;
  if (half === "pm" && hour < 12) hour += 12;
  if (half === "am" && hour === 12) hour = 0;
  return { hour, minute };
}

/** "today", "tomorrow" or a weekday ("fri", "friday"): the next such day, never today for a weekday. */
function parseDay(word: string, now: Date): Date | null {
  if (word === "today") return now;
  if (word === "tomorrow" || word === "tmr" || word === "tmrw") return daysAfter(now, 1);
  if (word.length < 3) return null;
  const weekday = WEEKDAYS.findIndex((name) => name.startsWith(word));
  return weekday === -1 ? null : nextWeekday(now, weekday);
}

/**
 * A typed time, or null if it isn't one: "in 2h", "3pm", "tomorrow 9",
 * "fri", "friday 3:30pm". A time alone is today, or tomorrow once it has
 * passed; a day alone is at 9 in the morning. Small on purpose, so it's
 * always clear what a phrase means.
 */
export function parseReminder(typed: string, now: Date): Date | null {
  const text = typed.trim().toLowerCase().replace(/\s+/g, " ");
  if (!text) return null;
  const delayed = parseDelay(text, now);
  if (delayed) return delayed;
  const [first, ...rest] = text.split(" ");
  const day = parseDay(first, now);
  if (day) {
    if (rest.length === 0) return atHour(day, MORNING_HOUR);
    const time = parseTimeOfDay(rest.join(" ").replace(/^at /, ""));
    return time ? atHour(day, time.hour, time.minute) : null;
  }
  const time = parseTimeOfDay(text.replace(/^at /, ""));
  if (!time) return null;
  const today = atHour(now, time.hour, time.minute);
  return today > now ? today : daysAfter(today, 1);
}

/** `day` at the time of day `time` gives. */
export function onDayAt(day: Date, time: { hour: number; minute: number }): Date {
  return atHour(day, time.hour, time.minute);
}

/** "Thursday, Nov 27 at 3:00 PM": a reminder time in full, for confirming a choice. */
export function describeReminderInFull(at: Date): string {
  const day = at.toLocaleDateString([], { weekday: "long", month: "short", day: "numeric" });
  return `${day} at ${formatTime(at.getTime())}`;
}

/** "Overdue", "Today 3:00 PM", "Tomorrow 9:00 AM", or "Mon, Nov 24 9:00 AM". */
export function describeReminder(remindMs: number, now: Date): string {
  if (remindMs <= now.getTime()) return "Overdue";
  const at = new Date(remindMs);
  const time = formatTime(remindMs);
  const tomorrow = daysAfter(now, 1);
  if (at.toDateString() === now.toDateString()) return `Today ${time}`;
  if (at.toDateString() === tomorrow.toDateString()) return `Tomorrow ${time}`;
  return `${at.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" })} ${time}`;
}
