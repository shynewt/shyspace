import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";

const SOURCE = fileURLToPath(new URL("../../src", import.meta.url));
const FEATURES = join(SOURCE, "features");

const PALETTE =
  "slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose";
const RAW_COLOR = new RegExp(
  [
    `\\b(?:bg|text|border|ring|outline|fill|stroke|decoration|from|via|to|shadow|divide|caret|accent)-(?:white|black|(?:${PALETTE})-\\d{2,3})\\b`,
    "\\[#[0-9a-fA-F]{3,8}\\]",
    "[\"'`]#(?:[0-9a-fA-F]{3}){1,2}(?:[0-9a-fA-F]{2})?[\"'`]",
    "\\b(?:rgba?|hsla?|oklch)\\(",
  ].join("|"),
  "g",
);
/** A native button's opening tag whose classes give it a solid semantic fill. */
const FILLED_BUTTON = /<button\b(?:=>|[^>])*?\bbg-(?:accent|danger|success|warning)(?![-/\w])/g;

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

function matchesIn(directory: string, pattern: RegExp): string[] {
  return sourceFiles(directory).flatMap((path) => {
    const text = readFileSync(path, "utf8");
    return [...text.matchAll(pattern)].map((match) => {
      const line = text.slice(0, match.index).split("\n").length;
      return `${path.slice(SOURCE.length + 1)}:${line} ${match[0].split("\n")[0]}`;
    });
  });
}

test("font sizes come from the type scale", () => {
  expect(matchesIn(SOURCE, /text-\[[^\]]*\]/g)).toEqual([]);
});

test("screens take their colours from the tokens", () => {
  expect(matchesIn(FEATURES, RAW_COLOR)).toEqual([]);
});

test("screens use Button for filled buttons", () => {
  expect(matchesIn(FEATURES, FILLED_BUTTON)).toEqual([]);
});
