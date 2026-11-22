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

/** A number of seconds written into a transition, outside a looping one (`repeat`). */
const INLINE_DURATION = /\{[^{}]*\b(?:duration|visualDuration):\s*[\d.]+[^{}]*\}/g;
/** Tailwind's numbered or arbitrary durations; the named ones (`duration-fast`) come from the motion tokens. */
const DURATION_CLASS = /\bduration-(?:\d+|\[[^\]]*\])/g;
/** A Motion target that animates a size, which lays the page out again on every frame. */
const SIZE_ANIMATION =
  /\b(?:initial|animate|exit|while[A-Z]\w*)(?:=\{\{|:\s*\{)[^}]*?\b(?:width|height|min[WH]\w*|max[WH]\w*):/g;
/** Only a row leaving a list may collapse its height, so the rows below close the gap it leaves. */
const SIZE_ANIMATION_ALLOWED = /^exit=\{\{/;
const ROWS_THAT_COLLAPSE_AS_THEY_LEAVE = [
  "features/inbox/InboxView.tsx",
  "features/later/LaterView.tsx",
  "features/members/JoinRequests.tsx",
];

function inlineDurations(text: string): string[] {
  return [...text.matchAll(INLINE_DURATION)].map((match) => match[0]).filter((found) => !/\brepeat:/.test(found));
}

function sizeAnimations(file: string, text: string): string[] {
  const isAllowed = (found: string) =>
    ROWS_THAT_COLLAPSE_AS_THEY_LEAVE.includes(file) && SIZE_ANIMATION_ALLOWED.test(found);
  return [...text.matchAll(SIZE_ANIMATION)].map((match) => match[0]).filter((found) => !isAllowed(found));
}

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

function relative(path: string): string {
  return path.slice(SOURCE.length + 1);
}

function matchesIn(directory: string, pattern: RegExp): string[] {
  return sourceFiles(directory).flatMap((path) => {
    const text = readFileSync(path, "utf8");
    return [...text.matchAll(pattern)].map((match) => {
      const line = text.slice(0, match.index).split("\n").length;
      return `${relative(path)}:${line} ${match[0].split("\n")[0]}`;
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

test("transitions take their durations from the motion tokens", () => {
  const found = sourceFiles(SOURCE)
    .filter((path) => !path.endsWith(join("ui", "motion.ts")))
    .flatMap((path) => inlineDurations(readFileSync(path, "utf8")).map((match) => `${relative(path)} ${match}`));
  expect(found).toEqual([]);
});

test("the duration check catches numbers but not tokens or loops", () => {
  expect(inlineDurations("transition={{ duration: 0.2 }}")).toHaveLength(1);
  expect(inlineDurations('exit={{ opacity: 0, transition: { duration: 0.22, ease: "easeIn" } }}')).toHaveLength(1);
  expect(inlineDurations('transition={{ type: "spring", visualDuration: 0.3 }}')).toHaveLength(1);
  expect(inlineDurations("transition={{ duration: durations.fast }}")).toEqual([]);
  expect(inlineDurations("transition={{ duration: 1.6, repeat: Number.POSITIVE_INFINITY }}")).toEqual([]);
});

test("CSS transitions use the named durations", () => {
  const found = sourceFiles(SOURCE).flatMap((path) =>
    [...readFileSync(path, "utf8").matchAll(DURATION_CLASS)].map((match) => `${relative(path)} ${match[0]}`),
  );
  expect(found).toEqual([]);
});

test("the duration class check catches numbers but not names", () => {
  const classes = (text: string) => [...text.matchAll(DURATION_CLASS)].map((match) => match[0]);
  expect(classes('className="transition-colors duration-500"')).toEqual(["duration-500"]);
  expect(classes('className="duration-[250ms]"')).toEqual(["duration-[250ms]"]);
  expect(classes('className="transition-opacity duration-highlight"')).toEqual([]);
});

test("Motion animates transforms and opacity, not sizes", () => {
  const found = sourceFiles(SOURCE).flatMap((path) =>
    sizeAnimations(relative(path), readFileSync(path, "utf8")).map((match) => `${relative(path)} ${match}`),
  );
  expect(found).toEqual([]);
});

test("the size check catches widths and heights, and allows a leaving row to collapse", () => {
  const sidebar = 'initial={{ width: 0 }} animate={{ width: "auto" }}';
  expect(sizeAnimations("features/shell/ShellLayout.tsx", sidebar)).toHaveLength(2);
  expect(sizeAnimations("features/later/X.tsx", "const reveal = { initial: { opacity: 0, height: 0 } };")).toHaveLength(
    1,
  );
  expect(sizeAnimations("features/later/LaterView.tsx", "exit={{ opacity: 0, height: 0 }}")).toEqual([]);
  expect(sizeAnimations("features/later/LaterView.tsx", 'animate={{ height: "auto" }}')).toHaveLength(1);
  expect(sizeAnimations("features/chat/X.tsx", "style={{ width: size, height: size }} animate={{ x: 0 }}")).toEqual([]);
});
