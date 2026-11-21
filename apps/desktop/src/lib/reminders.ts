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
const MONTHS = [
  "january",
  "february",
  "march",
  "april",
  "may",
  "june",
  "july",
  "august",
  "september",
  "october",
  "november",
  "december",
];
/** Each unit matches its abbreviations exactly, or any start of its full name ("min", "minu", "minutes"). */
const UNITS: { name: string; abbreviations: string[]; ms: number }[] = [
  { name: "minutes", abbreviations: ["m", "min", "mins"], ms: MINUTE_MS },
  { name: "hours", abbreviations: ["h", "hr", "hrs"], ms: HOUR_MS },
  { name: "days", abbreviations: ["d"], ms: DAY_MS },
  { name: "weeks", abbreviations: ["w", "wk", "wks"], ms: 7 * DAY_MS },
];

function unitMs(word: string): number | null {
  const unit = UNITS.find(({ name, abbreviations }) => abbreviations.includes(word) || name.startsWith(word));
  return unit ? unit.ms : null;
}

/** "5min", "5 minutes", "in 2h", "an hour", "3 days": a delay from now, with or without "in". */
function parseDelay(text: string, now: Date): Date | null {
  const match = /^(?:in )?(\d{1,3}|an?) ?([a-z]+)$/.exec(text);
  if (!match) return null;
  const count = /^\d/.test(match[1]) ? Number(match[1]) : 1;
  const ms = unitMs(match[2]);
  return ms && count > 0 ? new Date(now.getTime() + count * ms) : null;
}

type Clock = { hour: number; minute: number; isAmbiguous: boolean };

/** "9", "9am", "9:30 pm", "15:00", "noon". `isAmbiguous` when "5:30" could be morning or afternoon. */
function parseClock(typed: string): Clock | null {
  const text = typed.trim().toLowerCase();
  if (text === "noon") return { hour: 12, minute: 0, isAmbiguous: false };
  const match = /^(\d{1,2})(?::(\d{2}))?\s*(am|pm)?$/.exec(text);
  if (!match) return null;
  let hour = Number(match[1]);
  const minute = match[2] ? Number(match[2]) : 0;
  const half = match[3];
  if (minute > 59 || hour > 23 || (half && (hour === 0 || hour > 12))) return null;
  if (half === "pm" && hour < 12) hour += 12;
  if (half === "am" && hour === 12) hour = 0;
  const isAmbiguous = !half && hour >= 1 && hour <= 12 && !match[1].startsWith("0");
  return { hour, minute, isAmbiguous };
}

/** "9", "9am", "9:30 pm", "15:00", "noon": hours and minutes, or null. A bare hour reads as on a 24-hour clock. */
export function parseTimeOfDay(typed: string): { hour: number; minute: number } | null {
  const clock = parseClock(typed);
  return clock && { hour: clock.hour, minute: clock.minute };
}

/** The hours a clock could mean: 5 and 17 for an ambiguous "5". */
function hoursOf(clock: Clock): number[] {
  return clock.isAmbiguous ? [clock.hour % 12, (clock.hour % 12) + 12] : [clock.hour];
}

/** Each next time the clock shows `clock`, soonest first: "5:30" at 10am is 5:30 pm today, then 5:30 am tomorrow. */
function nextTimesShowing(clock: Clock, now: Date): Date[] {
  return hoursOf(clock)
    .map((hour) => {
      const today = atHour(now, hour, clock.minute);
      return today > now ? today : daysAfter(today, 1);
    })
    .sort((left, right) => left.getTime() - right.getTime());
}

const isDaytime = (hour: number) => hour >= MORNING_HOUR - 2 && hour < EVENING_HOUR;

function startsName(word: string, names: string[], shortest: number): number {
  return word.length < shortest ? -1 : names.findIndex((name) => name.startsWith(word));
}

/** "nov 30", "30 nov", "november 30th": that date this year, or next year once it has passed. */
function parseDate(first: string, second: string, now: Date): Date | null {
  const isDayFirst = /^\d/.test(first);
  const monthWord = isDayFirst ? second : first;
  const dayMatch = /^(\d{1,2})(?:st|nd|rd|th)?$/.exec(isDayFirst ? first : second);
  const month = startsName(monthWord, MONTHS, 3);
  if (!dayMatch || month === -1) return null;
  const dayOfMonth = Number(dayMatch[1]);
  for (const year of [now.getFullYear(), now.getFullYear() + 1]) {
    const date = new Date(year, month, dayOfMonth);
    if (date.getMonth() !== month) return null;
    if (date.toDateString() === now.toDateString() || date > now) return date;
  }
  return null;
}

/** The day the words start with, and the words left for a time: "today", "tom", "fri", "nov 30", "30 nov". */
function parseDay(words: string[], now: Date): { day: Date; rest: string[] } | null {
  const [first, second = "", ...others] = words;
  const date = parseDate(first, second, now);
  if (date) return { day: date, rest: others };
  const rest = words.slice(1);
  if (first === "today") return { day: now, rest };
  if (first === "tmr" || first === "tmrw" || startsName(first, ["tomorrow"], 3) === 0) {
    return { day: daysAfter(now, 1), rest };
  }
  const weekday = startsName(first, WEEKDAYS, 3);
  return weekday === -1 ? null : { day: nextWeekday(now, weekday), rest };
}

/**
 * A typed day and time: "3pm", "5:30", "tomorrow 9", "fri", "friday 3:30pm",
 * "nov 30 at 2pm". A day alone is at 9 in the morning. A time alone is the
 * next time the clock shows it; on a given day, daytime comes first.
 */
function parseMoment(text: string, now: Date): Date[] {
  const dated = parseDay(text.split(" "), now);
  if (!dated) {
    const clock = parseClock(text.replace(/^at /, ""));
    return clock ? nextTimesShowing(clock, now) : [];
  }
  if (dated.rest.length === 0) return [atHour(dated.day, MORNING_HOUR)];
  const clock = parseClock(dated.rest.join(" ").replace(/^at /, ""));
  if (!clock) return [];
  return hoursOf(clock)
    .sort((left, right) => Number(isDaytime(right)) - Number(isDaytime(left)))
    .map((hour) => atHour(dated.day, hour, clock.minute));
}

/** What a bare number could mean, likeliest first: "5" is in 5 minutes, in 5 hours, at 5, or in 5 days. */
function readingsOfNumber(count: number, now: Date): Date[] {
  const readings = [new Date(now.getTime() + count * MINUTE_MS)];
  if (count <= 99) readings.push(new Date(now.getTime() + count * HOUR_MS));
  const clock = parseClock(String(count));
  if (clock) readings.push(...nextTimesShowing(clock, now).slice(0, 1));
  if (count <= 365) readings.push(new Date(now.getTime() + count * DAY_MS));
  return readings;
}

/**
 * Every time `typed` could mean, likeliest first, or none if it isn't a time.
 * Some may already have passed ("today 8am" in the afternoon).
 */
export function reminderReadings(typed: string, now: Date): Date[] {
  const text = typed.trim().toLowerCase().replace(/\s+/g, " ");
  if (!text) return [];
  const bareNumber = /^(?:in )?(\d{1,3})$/.exec(text);
  if (bareNumber) return Number(bareNumber[1]) > 0 ? readingsOfNumber(Number(bareNumber[1]), now) : [];
  const delayed = parseDelay(text, now);
  return delayed ? [delayed] : parseMoment(text, now);
}

/** `day` at the time of day `time` gives. */
export function onDayAt(day: Date, time: { hour: number; minute: number }): Date {
  return atHour(day, time.hour, time.minute);
}

/** "Today at 3:00 PM", "Tomorrow at 9:00 AM", "Thursday, Nov 27 at 3:00 PM": a reminder time in full, for confirming a choice. */
export function describeReminderInFull(at: Date, now: Date): string {
  const time = formatTime(at.getTime());
  if (at.toDateString() === now.toDateString()) return `Today at ${time}`;
  if (at.toDateString() === daysAfter(now, 1).toDateString()) return `Tomorrow at ${time}`;
  const day = at.toLocaleDateString([], { weekday: "long", month: "short", day: "numeric" });
  return `${day} at ${time}`;
}

const DISTANCE_STEPS: { unit: Intl.RelativeTimeFormatUnit; ms: number; below: number }[] = [
  { unit: "minute", ms: MINUTE_MS, below: HOUR_MS },
  { unit: "hour", ms: HOUR_MS, below: DAY_MS },
  { unit: "day", ms: DAY_MS, below: 14 * DAY_MS },
  { unit: "week", ms: 7 * DAY_MS, below: 61 * DAY_MS },
  { unit: "month", ms: 30 * DAY_MS, below: Number.POSITIVE_INFINITY },
];

/** "in 5 minutes", "in 3 hours", "in 2 weeks": how far off `at` is, roughly. */
export function describeDistance(at: Date, now: Date): string {
  const ms = Math.max(at.getTime() - now.getTime(), MINUTE_MS);
  const step = DISTANCE_STEPS.find(({ below }) => ms < below) ?? DISTANCE_STEPS[DISTANCE_STEPS.length - 1];
  return new Intl.RelativeTimeFormat([], { numeric: "always" }).format(Math.round(ms / step.ms), step.unit);
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
