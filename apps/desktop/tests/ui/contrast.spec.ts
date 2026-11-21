import { expect, type Page, test } from "@playwright/test";

/** WCAG AA for normal-size text. */
const AA_TEXT = 4.5;
/** WCAG AA for icons and other meaningful graphics. */
const AA_GRAPHIC = 3;

type Color = string | { mix: string; percent: number; over: string };

/** Text colors and what they sit on. A `mix` is a tinted fill like `bg-danger/15`. */
const TEXT_PAIRS: [string, Color][] = [
  ["--text", "--surface"],
  ["--text-muted", "--surface"],
  ["--text-muted", "--bg"],
  ["--text-muted", "--surface-2"],
  ["--text-faint", "--surface"],
  ["--text-faint", "--bg"],
  ["--accent-text", "--surface"],
  ["--accent-text", "--bg"],
  ["--accent-text", "--accent-soft"],
  ["--accent-text", { mix: "--accent", percent: 10, over: "--surface" }],
  ["--danger-text", "--surface"],
  ["--danger-text", "--bg"],
  ["--danger-text", { mix: "--danger", percent: 15, over: "--surface" }],
  ["--warning-text", "--surface"],
  ["--warning-text", { mix: "--warning", percent: 15, over: "--surface" }],
  ["--success-text", "--surface"],
  ["--success-text", { mix: "--success", percent: 12, over: "--surface" }],
  ["--on-accent", "--accent"],
  ["--on-accent", "--accent-strong"],
  ["#ffffff", "--danger"],
];

const GRAPHIC_PAIRS: [string, string][] = [
  ["--accent", "--surface"],
  ["--accent", "--bg"],
  ["--accent", "--accent-soft"],
  // The focus ring inside a hovered row.
  ["--accent", "--surface-2"],
];

const ACCENTS = ["violet", "blue", "indigo", "teal", "pink", "graphite"];

function channels(hex: string): number[] {
  const match = hex.trim().match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i);
  if (!match) throw new Error(`not a #rrggbb color: "${hex}"`);
  return match.slice(1).map((part) => Number.parseInt(part, 16) / 255);
}

function luminance(hex: string): number {
  const [red, green, blue] = channels(hex).map((value) =>
    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4,
  );
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}

function contrast(foreground: string, background: string): number {
  const [lighter, darker] = [luminance(foreground), luminance(background)].sort((left, right) => right - left);
  return (lighter + 0.05) / (darker + 0.05);
}

function hexByte(value: number): string {
  return Math.round(value * 255)
    .toString(16)
    .padStart(2, "0");
}

function blend(top: string, percent: number, bottom: string): string {
  const over = channels(bottom);
  const mixed = channels(top).map((value, index) => (value * percent + over[index] * (100 - percent)) / 100);
  return `#${mixed.map(hexByte).join("")}`;
}

function tokenNames(color: Color): string[] {
  if (typeof color !== "string") return [...tokenNames(color.mix), ...tokenNames(color.over)];
  return color.startsWith("--") ? [color] : [];
}

async function tokens(page: Page): Promise<(color: Color) => string> {
  const names = [...TEXT_PAIRS, ...GRAPHIC_PAIRS].flatMap((pair) => pair.flatMap(tokenNames));
  const values = await page.evaluate((names) => {
    const style = getComputedStyle(document.documentElement);
    return Object.fromEntries(names.map((name) => [name, style.getPropertyValue(name).trim()]));
  }, names);
  const resolve = (color: Color): string => {
    if (typeof color !== "string") return blend(resolve(color.mix), color.percent, resolve(color.over));
    if (color.startsWith("#")) return color;
    const value = values[color];
    if (!value) throw new Error(`no token ${color}`);
    return value;
  };
  return resolve;
}

async function unreadablePairs(page: Page): Promise<string[]> {
  const resolve = await tokens(page);
  const failures: string[] = [];
  for (const [text, background] of TEXT_PAIRS) {
    const ratio = contrast(resolve(text), resolve(background));
    if (ratio < AA_TEXT) failures.push(`${text} on ${JSON.stringify(background)}: ${ratio.toFixed(2)}`);
  }
  for (const [graphic, background] of GRAPHIC_PAIRS) {
    const ratio = contrast(resolve(graphic), resolve(background));
    if (ratio < AA_GRAPHIC) failures.push(`${graphic} on ${background}: ${ratio.toFixed(2)}`);
  }
  return failures;
}

async function expectReadable(page: Page) {
  expect(await unreadablePairs(page)).toEqual([]);
}

async function openApp(page: Page) {
  await page.goto("");
  await page.getByRole("navigation", { name: "Workspace" }).waitFor();
}

async function rootToken(page: Page, name: string): Promise<string> {
  return page.evaluate((name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim(), name);
}

async function background(page: Page): Promise<string> {
  return rootToken(page, "--bg");
}

test("text meets WCAG AA in the light theme", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await openApp(page);
  await expectReadable(page);
});

test("text meets WCAG AA in the dark theme", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await openApp(page);
  await expectReadable(page);
});

test("a chosen theme wins over the system's", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await openApp(page);
  const light = await background(page);
  await page.emulateMedia({ colorScheme: "dark" });
  const dark = await background(page);
  expect(dark).not.toBe(light);

  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
  expect(await background(page)).toBe(light);
  await page.emulateMedia({ colorScheme: "light" });
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
  expect(await background(page)).toBe(dark);
  await expectReadable(page);
});

for (const scheme of ["light", "dark"] as const) {
  test(`every accent meets WCAG AA in the ${scheme} theme`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await openApp(page);
    const failures: string[] = [];
    const accents = new Set<string>();
    for (const accent of ACCENTS) {
      await page.evaluate((accent) => document.documentElement.setAttribute("data-accent", accent), accent);
      accents.add(await rootToken(page, "--accent"));
      failures.push(...(await unreadablePairs(page)).map((failure) => `${accent}: ${failure}`));
    }
    expect(failures).toEqual([]);
    expect(accents.size, "each accent has its own color").toBe(ACCENTS.length);
  });
}
