// Writes the phone apps' design tokens from the desktop stylesheet, so colours and motion have
// one source: apps/desktop/src/styles.css. `--check` fails if a written file is out of date.
// Type sizes are not exported: the phone apps use their platform's text styles.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(dirname(fileURLToPath(import.meta.url)), "../..");
const stylesheet = join(repo, "apps/desktop/src/styles.css");
const kotlinFile = join(repo, "apps/android/app/src/main/java/dev/shynewt/shyspace/ui/theme/ShyTokens.kt");

const PALETTE_KEYS = [
  "bg", "surface", "surface-2", "surface-3", "border",
  "text", "text-muted", "text-faint", "text-decorative",
  "danger", "danger-text", "success", "success-text", "warning", "warning-text",
];
const ACCENT_KEYS = ["accent", "accent-strong", "accent-soft", "accent-text", "on-accent"];
const DURATIONS = ["exit", "fast", "base", "panel", "highlight"];

/** The top-level blocks of `css`, each with its selector and body. */
function blocks(css) {
  const found = [];
  let depth = 0;
  let start = 0;
  let selectorStart = 0;
  for (let index = 0; index < css.length; index++) {
    const char = css[index];
    if (char === "{") {
      if (depth === 0) {
        start = index + 1;
        found.push({ selector: css.slice(selectorStart, index).trim() });
      }
      depth++;
    } else if (char === "}") {
      depth--;
      if (depth === 0) {
        found[found.length - 1].body = css.slice(start, index);
        selectorStart = index + 1;
      }
    } else if (depth === 0 && char === ";") {
      selectorStart = index + 1;
    }
  }
  return found;
}

/** The custom properties a block sets for light, and those its `@variant dark` sets. */
function sides(body) {
  const darkStart = body.indexOf("@variant dark");
  const light = darkStart === -1 ? body : body.slice(0, darkStart);
  const dark = darkStart === -1 ? "" : body.slice(darkStart, body.indexOf("}", darkStart));
  return { light: properties(light), dark: properties(dark) };
}

function properties(text) {
  const found = {};
  for (const match of text.matchAll(/--([\w-]+):\s*([^;]+);/g)) {
    found[match[1]] = match[2].trim();
  }
  return found;
}

function fail(message) {
  console.error(`design tokens: ${message}`);
  process.exit(1);
}

/** Each side merged as the cascade does; a block's dark side must cover what its light side sets. */
function resolve(base, own, name, keys) {
  for (const key of Object.keys(own.light)) {
    if (keys.includes(key) && !(key in own.dark) && key !== "on-accent") {
      fail(`${name} sets --${key} for light but not for dark`);
    }
  }
  const light = { ...base.light, ...own.light };
  const dark = { ...base.light, ...base.dark, ...own.light, ...own.dark };
  for (const [side, values] of [["light", light], ["dark", dark]]) {
    for (const key of keys) {
      if (!/^#[0-9a-f]{6}$/i.test(values[key] ?? "")) fail(`${name} ${side} --${key} isn't a #rrggbb colour`);
    }
  }
  return { light, dark };
}

function read() {
  const all = blocks(readFileSync(stylesheet, "utf8").replace(/\/\*[\s\S]*?\*\//g, ""));
  const base = all.find((block) => block.selector === ":root");
  if (!base) fail("no :root block");
  const baseSides = sides(base.body);

  const palettes = { shy: resolve({ light: {}, dark: {} }, baseSides, "shy", PALETTE_KEYS) };
  const accents = {};
  let violetSides;
  for (const block of all) {
    const palette = block.selector.match(/^\[data-palette="(\w+)"\]$/);
    if (palette) palettes[palette[1]] = resolve(palettes.shy, sides(block.body), palette[1], PALETTE_KEYS);
    const accent = block.selector.match(/\[data-accent="(\w+)"\]$/);
    if (accent) {
      const own = sides(block.body);
      if (accent[1] === "violet") violetSides = own;
      accents[accent[1]] = resolve(violetSides ?? { light: {}, dark: {} }, own, accent[1], ACCENT_KEYS);
    }
  }
  if (!violetSides) fail("no violet accent block");

  const motion = {};
  for (const block of all.filter((each) => each.selector.startsWith("@theme"))) {
    Object.assign(motion, properties(block.body));
  }
  const easing = motion["ease-out"]?.match(/^cubic-bezier\(([^)]+)\)$/);
  if (!easing) fail("no --ease-out cubic-bezier");
  const durations = {};
  for (const name of DURATIONS) {
    const value = motion[`transition-duration-${name}`]?.match(/^(\d+)ms$/);
    if (!value) fail(`no --transition-duration-${name} in ms`);
    durations[name] = Number(value[1]);
  }
  return { palettes, accents, easing: easing[1].split(",").map(Number), durations };
}

const camel = (name) => name.replace(/-(\w)/g, (_, letter) => letter.toUpperCase());
const constant = (name) => name.toUpperCase();
const kotlinColor = (hex) => `Color(0xFF${hex.slice(1).toUpperCase()})`;

function kotlin({ palettes, accents, easing, durations }) {
  const source = relative(repo, stylesheet);
  const colors = (values, keys, indent) =>
    keys.map((key) => `${indent}${camel(key)} = ${kotlinColor(values[key])},`).join("\n");
  const entries = (named, type, keys) =>
    Object.entries(named)
      .map(
        ([name, { light, dark }]) =>
          `    ${constant(name)}(\n        light = ${type}(\n${colors(light, keys, "            ")}\n        ),\n        dark = ${type}(\n${colors(dark, keys, "            ")}\n        ),\n    ),`,
      )
      .join("\n");
  const fields = (keys) => keys.map((key) => `    val ${camel(key)}: Color,`).join("\n");
  return `// Generated from ${source} by tools/design-tokens/export.mjs. Don't edit; run the script.
package dev.shynewt.shyspace.ui.theme

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.ui.graphics.Color

data class ShyColors(
${fields(PALETTE_KEYS)}
)

data class ShyAccentColors(
${fields(ACCENT_KEYS)}
)

enum class ShyPalette(val light: ShyColors, val dark: ShyColors) {
${entries(palettes, "ShyColors", PALETTE_KEYS)}
}

enum class ShyAccent(val light: ShyAccentColors, val dark: ShyAccentColors) {
${entries(accents, "ShyAccentColors", ACCENT_KEYS)}
}

object ShyMotion {
    val easeOut = CubicBezierEasing(${easing.map((value) => `${value}f`).join(", ")})
${DURATIONS.map((name) => `    const val ${camel(name)}Millis = ${durations[name]}`).join("\n")}
}
`;
}

const outputs = [[kotlinFile, kotlin(read())]];
const isCheck = process.argv.includes("--check");
let isStale = false;
for (const [path, text] of outputs) {
  let current = "";
  try {
    current = readFileSync(path, "utf8");
  } catch {}
  if (current === text) continue;
  if (isCheck) {
    console.error(`design tokens: ${relative(repo, path)} is out of date; run node tools/design-tokens/export.mjs`);
    isStale = true;
  } else {
    writeFileSync(path, text);
    console.log(`wrote ${relative(repo, path)}`);
  }
}
if (isStale) process.exit(1);
