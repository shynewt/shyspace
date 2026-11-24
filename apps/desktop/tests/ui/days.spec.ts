/** Day labels and the groups built from them, for days long ago. */
import { expect, test } from "@playwright/test";
import { formatDay, groupByDay } from "../../src/lib/people";

const year = new Date().getFullYear();
const day = (forYear: number) => new Date(forYear, 0, 15, 12).getTime();

test("a day in another year says its year, and one this year doesn't", () => {
  expect(formatDay(day(year - 1))).toContain(String(year - 1));
  // January 15 is today, yesterday or earlier this year; only the last case reads as a date.
  const thisYear = formatDay(day(year));
  if (thisYear !== "Today" && thisYear !== "Yesterday") expect(thisYear).not.toContain(String(year));
});

test("the same date in two different years makes two groups", () => {
  // January 15 was a Wednesday in both, so only the year tells them apart.
  const groups = groupByDay([day(2025), day(2020)], (time) => time);
  expect(groups).toHaveLength(2);
  expect(groups.map((group) => group.items.length)).toEqual([1, 1]);
});
