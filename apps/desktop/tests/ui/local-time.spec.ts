/** Other people's local time and working hours. Runs without a browser. */
import { expect, test } from "@playwright/test";
import {
  cityOf,
  describeDays,
  describeTimeDifference,
  formatOffset,
  isAfterHours,
  isKnownTimeZone,
  isWithinWorkingHours,
  offsetMinutes,
  wallClockIn,
} from "../../src/lib/localTime";

const MONDAY_NOON_UTC = new Date(Date.UTC(2026, 10, 23, 12, 0));
const WEEKDAYS_NINE_TO_FIVE = { days: 0b001_1111, start_minute: 9 * 60, end_minute: 17 * 60 };
/** Fri, Sat and Sun nights, 22:00 to 06:00. */
const WEEKEND_NIGHTS = { days: 0b111_0000, start_minute: 22 * 60, end_minute: 6 * 60 };

test("reads the clock in another zone, daylight saving included", () => {
  expect(wallClockIn("Asia/Tokyo", MONDAY_NOON_UTC)).toEqual({ weekday: 0, minute: 21 * 60 });
  expect(wallClockIn("America/Los_Angeles", MONDAY_NOON_UTC)).toEqual({ weekday: 0, minute: 4 * 60 });
  expect(offsetMinutes("Asia/Kolkata", MONDAY_NOON_UTC)).toBe(330);
  expect(offsetMinutes("Europe/Berlin", new Date(Date.UTC(2026, 6, 1)))).toBe(120);
  expect(offsetMinutes("Europe/Berlin", MONDAY_NOON_UTC)).toBe(60);
});

test("names offsets and cities the way the picker shows them", () => {
  expect(formatOffset(0)).toBe("GMT+0");
  expect(formatOffset(330)).toBe("GMT+5:30");
  expect(formatOffset(-210)).toBe("GMT-3:30");
  expect(cityOf("America/Argentina/Buenos_Aires")).toBe("Buenos Aires");
  expect(cityOf("UTC")).toBe("UTC");
  expect(cityOf("Asia/Calcutta")).toBe("Kolkata");
});

test("says how far ahead or behind someone is, or nothing in the same time", () => {
  expect(describeTimeDifference("Asia/Tokyo", "Europe/London", MONDAY_NOON_UTC)).toBe("9 h ahead");
  expect(describeTimeDifference("Asia/Kolkata", "Europe/Berlin", MONDAY_NOON_UTC)).toBe("4 h 30 min ahead");
  expect(describeTimeDifference("America/St_Johns", "UTC", MONDAY_NOON_UTC)).toBe("3 h 30 min behind");
  expect(describeTimeDifference("Europe/Lisbon", "Europe/London", MONDAY_NOON_UTC)).toBeNull();
});

test("working hours count only on their days", () => {
  const at = (weekday: number, hour: number, minute = 0) => ({ weekday, minute: hour * 60 + minute });
  expect(isWithinWorkingHours(WEEKDAYS_NINE_TO_FIVE, at(0, 9))).toBe(true);
  expect(isWithinWorkingHours(WEEKDAYS_NINE_TO_FIVE, at(4, 16, 59))).toBe(true);
  expect(isWithinWorkingHours(WEEKDAYS_NINE_TO_FIVE, at(4, 17))).toBe(false);
  expect(isWithinWorkingHours(WEEKDAYS_NINE_TO_FIVE, at(0, 8, 59))).toBe(false);
  expect(isWithinWorkingHours(WEEKDAYS_NINE_TO_FIVE, at(5, 12))).toBe(false);
});

test("hours that cross midnight belong to the day they start", () => {
  const at = (weekday: number, hour: number) => ({ weekday, minute: hour * 60 });
  expect(isWithinWorkingHours(WEEKEND_NIGHTS, at(4, 23))).toBe(true);
  expect(isWithinWorkingHours(WEEKEND_NIGHTS, at(5, 3)), "Friday's night runs into Saturday").toBe(true);
  expect(isWithinWorkingHours(WEEKEND_NIGHTS, at(0, 3)), "Sunday's night runs into Monday").toBe(true);
  expect(isWithinWorkingHours(WEEKEND_NIGHTS, at(4, 3)), "Thursday isn't a working night").toBe(false);
  expect(isWithinWorkingHours(WEEKEND_NIGHTS, at(0, 23))).toBe(false);
  expect(isWithinWorkingHours(WEEKEND_NIGHTS, at(5, 12))).toBe(false);
});

test("after hours run from 20:00 to 08:00", () => {
  expect(isAfterHours({ weekday: 0, minute: 19 * 60 + 59 })).toBe(false);
  expect(isAfterHours({ weekday: 0, minute: 20 * 60 })).toBe(true);
  expect(isAfterHours({ weekday: 0, minute: 7 * 60 + 59 })).toBe(true);
  expect(isAfterHours({ weekday: 0, minute: 8 * 60 })).toBe(false);
});

test("days read as runs", () => {
  expect(describeDays(0b111_1111)).toBe("Every day");
  expect(describeDays(0b001_1111)).toMatch(/^Mon–Fri$/);
  expect(describeDays(0b001_0101)).toBe("Mon, Wed, Fri");
  expect(describeDays(0b010_0011)).toBe("Mon, Tue, Sat");
});

test("unknown zones are known as unknown", () => {
  expect(isKnownTimeZone("Europe/Lisbon")).toBe(true);
  expect(isKnownTimeZone("Mars/Olympus_Mons")).toBe(false);
  expect(isKnownTimeZone("")).toBe(false);
});
