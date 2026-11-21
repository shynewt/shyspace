/**
 * Other people's time: what the clock says where they are, how far that is
 * from yours, and whether it's within the working hours they shared. Zones
 * are IANA names; one this device doesn't know shows no local time.
 */
import type { WorkingHours } from "./api";
import { formatTime } from "./people";

export const MINUTES_PER_DAY = 24 * 60;
/** From 22:00 to 07:00 it's night, whatever someone's working hours. */
const NIGHT_STARTS = 22 * 60;
const NIGHT_ENDS = 7 * 60;
const SHORT_WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** What a clock in some zone shows: the day of the week (0 is Monday) and the minute of the day. */
export type WallClock = { weekday: number; minute: number };

const formatters = new Map<string, Intl.DateTimeFormat>();

/** A formatter that reads a moment's date and time in `zone`; throws for a zone this device doesn't know. */
function partsFormatter(zone: string): Intl.DateTimeFormat {
  let formatter = formatters.get(zone);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat("en-US", {
      timeZone: zone,
      hourCycle: "h23",
      weekday: "short",
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "numeric",
    });
    formatters.set(zone, formatter);
  }
  return formatter;
}

export function isKnownTimeZone(zone: string): boolean {
  if (!zone) return false;
  try {
    partsFormatter(zone);
    return true;
  } catch {
    return false;
  }
}

export function deviceTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}

/** Every zone this device knows, with the device's own even if the list leaves it out (like "UTC"). */
export function timeZoneNames(): string[] {
  const names = Intl.supportedValuesOf("timeZone");
  const own = deviceTimeZone();
  return names.includes(own) ? names : [own, ...names];
}

function readParts(zone: string, at: Date) {
  const parts = Object.fromEntries(
    partsFormatter(zone)
      .formatToParts(at)
      .map((part) => [part.type, part.value]),
  );
  return {
    weekday: SHORT_WEEKDAYS.indexOf(parts.weekday),
    year: Number(parts.year),
    month: Number(parts.month),
    day: Number(parts.day),
    hour: Number(parts.hour),
    minute: Number(parts.minute),
  };
}

export function wallClockIn(zone: string, at: Date): WallClock {
  const parts = readParts(zone, at);
  return { weekday: parts.weekday, minute: parts.hour * 60 + parts.minute };
}

/** How far `zone`'s clocks are ahead of UTC at `at`, in minutes. */
export function offsetMinutes(zone: string, at: Date): number {
  const parts = readParts(zone, at);
  const asIfUtc = Date.UTC(parts.year, parts.month - 1, parts.day, parts.hour, parts.minute);
  const wholeMinute = Math.floor(at.getTime() / 60_000) * 60_000;
  return Math.round((asIfUtc - wholeMinute) / 60_000);
}

/** "GMT+1", "GMT-3:30", "GMT+0". */
export function formatOffset(minutes: number): string {
  const sign = minutes < 0 ? "-" : "+";
  const hours = Math.floor(Math.abs(minutes) / 60);
  const rest = Math.abs(minutes) % 60;
  return `GMT${sign}${hours}${rest ? `:${String(rest).padStart(2, "0")}` : ""}`;
}

/** Cities renamed since some engines' zone lists were written ("Asia/Calcutta" in Chromium). */
const RENAMED_CITIES: Record<string, string> = {
  "Asia/Calcutta": "Kolkata",
  "Asia/Saigon": "Ho Chi Minh City",
  "Asia/Katmandu": "Kathmandu",
  "Asia/Rangoon": "Yangon",
  "Europe/Kiev": "Kyiv",
};

/** The place a zone is named after: "Buenos Aires" for "America/Argentina/Buenos_Aires". */
export function cityOf(zone: string): string {
  return RENAMED_CITIES[zone] ?? (zone.split("/").pop() ?? zone).replaceAll("_", " ");
}

/** "11:42 PM" as a clock in `zone` shows it, in this device's format. */
export function formatTimeIn(zone: string, at: Date): string {
  return at.toLocaleTimeString([], { hour: "numeric", minute: "2-digit", timeZone: zone });
}

function describeMinutes(minutes: number): string {
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return [hours && `${hours} h`, rest && `${rest} min`].filter(Boolean).join(" ");
}

/** "8 h ahead", "5 h 30 min behind", or null when their clocks show the same time as yours. */
export function describeTimeDifference(theirZone: string, myZone: string, at: Date): string | null {
  const difference = offsetMinutes(theirZone, at) - offsetMinutes(myZone, at);
  if (difference === 0) return null;
  return `${describeMinutes(Math.abs(difference))} ${difference > 0 ? "ahead" : "behind"}`;
}

function worksOn(hours: WorkingHours, weekday: number): boolean {
  return (hours.days & (1 << weekday)) !== 0;
}

/** Whether `clock` falls in their hours; hours that end at or before they start run past midnight into the next day. */
export function isWithinWorkingHours(hours: WorkingHours, clock: WallClock): boolean {
  const { start_minute: start, end_minute: end } = hours;
  if (start < end) return worksOn(hours, clock.weekday) && clock.minute >= start && clock.minute < end;
  const previousDay = (clock.weekday + 6) % 7;
  return (
    (worksOn(hours, clock.weekday) && clock.minute >= start) || (worksOn(hours, previousDay) && clock.minute < end)
  );
}

export function isNight(clock: WallClock): boolean {
  return clock.minute >= NIGHT_STARTS || clock.minute < NIGHT_ENDS;
}

/** "9:00 AM" or "09:00", as this device writes times; 24:00 reads as midnight. */
export function formatMinuteOfDay(minute: number): string {
  const at = new Date(2024, 0, 1, 0, minute % MINUTES_PER_DAY);
  return formatTime(at.getTime());
}

/** "Mon" to "Sun" in this device's language. 1 January 2024 was a Monday. */
export function weekdayName(weekday: number, width: "short" | "long"): string {
  return new Date(Date.UTC(2024, 0, 1 + weekday)).toLocaleDateString([], { weekday: width, timeZone: "UTC" });
}

/** "Every day", "Mon–Fri", "Mon, Wed, Fri" or "Mon–Wed, Sat". */
export function describeDays(days: number): string {
  if ((days & 0b111_1111) === 0b111_1111) return "Every day";
  const runs: [number, number][] = [];
  for (let weekday = 0; weekday < 7; weekday++) {
    if ((days & (1 << weekday)) === 0) continue;
    const last = runs.at(-1);
    if (last && last[1] === weekday - 1) last[1] = weekday;
    else runs.push([weekday, weekday]);
  }
  return runs
    .flatMap(([first, last]) => {
      const [from, to] = [weekdayName(first, "short"), weekdayName(last, "short")];
      if (first === last) return [from];
      return last === first + 1 ? [from, to] : [`${from}–${to}`];
    })
    .join(", ");
}

/** "Mon–Fri, 9:00 AM–5:00 PM". */
export function describeWorkingHours(hours: WorkingHours): string {
  return `${describeDays(hours.days)}, ${formatMinuteOfDay(hours.start_minute)}–${formatMinuteOfDay(hours.end_minute)}`;
}
