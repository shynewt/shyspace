import { expect, type Page, test } from "@playwright/test";

/** How an element looked in one animation frame. */
type Frame = { at: number; isPresent: boolean; opacity: number; transform: string; left: number; top: number };

declare global {
  interface Window {
    motionSampling?: Promise<Frame[]>;
  }
}

/** Generous: the longest entrance (a dialog) takes 150 ms, plus frames lost to a busy test machine. */
const ENTRANCE_BUDGET_MS = 250;
/**
 * A busy test machine sometimes stops drawing frames for a while. Motion
 * catches up after such a pause, so a gap between frames counts as at most
 * this much: the timings measure the animation, not the machine.
 */
const LONGEST_COUNTED_FRAME_MS = 50;
const EXIT_BUDGET_MS = 200;
const IDENTITY = ["none", "matrix(1, 0, 0, 1, 0, 0)"];

async function openApp(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
}

async function sendMessage(page: Page, text: string) {
  await page.getByLabel("Message #general").fill(text);
  await page.getByLabel("Message #general").press("Enter");
  const message = page.getByRole("log", { name: "Messages in #general" }).getByRole("article").last();
  await expect(message).toContainText(text);
  return message;
}

/** Starts recording, every frame for `ms`, the first element matching `selector`. */
async function startSampling(page: Page, selector: string, ms = 2000) {
  await page.evaluate(
    ({ selector, ms }) => {
      window.motionSampling = new Promise((resolve) => {
        const frames: Frame[] = [];
        const start = performance.now();
        const sample = () => {
          const at = performance.now() - start;
          const element = document.querySelector(selector);
          if (element) {
            const style = getComputedStyle(element);
            const box = element.getBoundingClientRect();
            frames.push({
              at,
              isPresent: true,
              opacity: Number(style.opacity),
              transform: style.transform,
              left: box.left,
              top: box.top,
            });
          } else {
            frames.push({ at, isPresent: false, opacity: 0, transform: "none", left: 0, top: 0 });
          }
          if (at < ms) requestAnimationFrame(sample);
          else resolve(frames);
        };
        requestAnimationFrame(sample);
      });
    },
    { selector, ms },
  );
}

async function sampledFrames(page: Page): Promise<Frame[]> {
  return page.evaluate(() => {
    if (!window.motionSampling) throw new Error("not sampling");
    return window.motionSampling;
  });
}

function isSettled(frame: Frame): boolean {
  return frame.isPresent && frame.opacity === 1 && IDENTITY.includes(frame.transform);
}

/** Within what anyone could see of fully opaque at its own size: an ease-out's last steps are too small to see. */
function looksSettled(frame: Frame): boolean {
  const scale = frame.transform.startsWith("matrix(") ? Number.parseFloat(frame.transform.slice(7)) : 1;
  return frame.isPresent && frame.opacity >= 0.99 && Math.abs(scale - 1) < 0.002;
}

/** The time from frame `from` to frame `to`, counting each gap between frames as at most `LONGEST_COUNTED_FRAME_MS`. */
function timeBetween(frames: Frame[], from: number, to: number): number {
  if (from < 0 || to < from) throw new Error(`no such frames (${from} to ${to}): ${JSON.stringify(frames)}`);
  let total = 0;
  for (let index = from + 1; index <= to; index++) {
    total += Math.min(frames[index].at - frames[index - 1].at, LONGEST_COUNTED_FRAME_MS);
  }
  return total;
}

/** From the first frame the element is there to the first where it looks settled. */
function entranceTime(frames: Frame[]): number {
  return timeBetween(
    frames,
    frames.findIndex((frame) => frame.isPresent),
    frames.findIndex(looksSettled),
  );
}

/** From the first frame the element starts leaving (or is gone) to the first where it's gone. */
function exitTime(frames: Frame[]): number {
  const leaving = frames.findIndex((frame) => !isSettled(frame));
  return timeBetween(
    frames,
    leaving,
    frames.findIndex((frame, index) => index >= leaving && !frame.isPresent),
  );
}

/** Presses a key in the page itself, then waits `frames` animation frames, so no frame is missed. */
async function pressAndWaitFrames(page: Page, key: { key: string; ctrlKey?: boolean; altKey?: boolean }, frames = 2) {
  await page.evaluate(
    async ({ key, frames }) => {
      const target = document.activeElement ?? document.body;
      target.dispatchEvent(new KeyboardEvent("keydown", { ...key, bubbles: true, cancelable: true }));
      for (let frame = 0; frame < frames; frame++) await new Promise(requestAnimationFrame);
    },
    { key, frames },
  );
}

test.describe("floating surfaces", () => {
  test.beforeEach(async ({ page }) => openApp(page));

  test("the palette opens and closes at once", async ({ page }) => {
    await pressAndWaitFrames(page, { key: "k", ctrlKey: true });
    const palette = page.getByRole("dialog", { name: "Search" });
    expect(await palette.evaluate((element) => getComputedStyle(element).opacity)).toBe("1");
    expect(IDENTITY).toContain(await palette.evaluate((element) => getComputedStyle(element).transform));

    await pressAndWaitFrames(page, { key: "Escape" });
    await expect(palette).toHaveCount(0, { timeout: 1 });
  });

  test("a tooltip beside one just shown appears at once", async ({ page }) => {
    const message = await sendMessage(page, "Hello");
    await message.hover();
    await message.getByRole("button", { name: "React" }).hover();
    await expect(page.getByRole("tooltip").filter({ hasText: "React" })).toBeVisible();
    await startSampling(page, '[data-radix-popper-content-wrapper] > [data-state="instant-open"]');

    // Down off the button first, away from its tooltip above, as a straight move might look like heading for it.
    const react = await message.getByRole("button", { name: "React" }).boundingBox();
    const reply = await message.getByRole("button", { name: "Reply in thread" }).boundingBox();
    if (!react || !reply) throw new Error("no toolbar buttons");
    await page.mouse.move(react.x + react.width / 2, react.y + react.height + 6, { steps: 2 });
    await page.mouse.move(reply.x + reply.width / 2, reply.y + reply.height / 2, { steps: 2 });
    await expect(page.getByRole("tooltip").filter({ hasText: "Reply in thread" })).toBeVisible();
    const first = (await sampledFrames(page)).find((frame) => frame.isPresent);
    expect(first && isSettled(first)).toBe(true);
  });

  test("a menu opens within its budget and leaves soon", async ({ page }) => {
    await startSampling(page, '[role="menu"]');
    await page.getByRole("button", { name: /^Ada\b/ }).click();
    await expect(page.getByRole("menu")).toBeVisible();
    expect(entranceTime(await sampledFrames(page))).toBeLessThanOrEqual(ENTRANCE_BUDGET_MS);

    await startSampling(page, '[role="menu"]');
    await page.keyboard.press("Escape");
    expect(exitTime(await sampledFrames(page))).toBeLessThanOrEqual(EXIT_BUDGET_MS);
  });

  test("a popover opens within its budget and grows from its trigger", async ({ page }) => {
    const message = await sendMessage(page, "Hello");
    await message.hover();
    const popover = "[data-radix-popper-content-wrapper] > *";
    await startSampling(page, popover);

    await message.getByRole("button", { name: "React" }).click();
    await expect(page.getByRole("button", { name: "React with 👍" })).toBeVisible();
    expect(entranceTime(await sampledFrames(page))).toBeLessThanOrEqual(ENTRANCE_BUDGET_MS);
    // Above the button and aligned to its end: it grows from its bottom right corner, not its middle.
    const { origin, width, height } = await page.locator(popover).evaluate((element) => ({
      origin: getComputedStyle(element).transformOrigin.split(" ").map(Number.parseFloat),
      width: element.getBoundingClientRect().width,
      height: element.getBoundingClientRect().height,
    }));
    expect(origin[0]).toBeCloseTo(width, 0);
    expect(origin[1]).toBeCloseTo(height, 0);
  });

  test("a dialog opens within its budget and leaves soon", async ({ page }) => {
    await startSampling(page, '[role="dialog"]');
    await page.keyboard.press("Control+/");
    await expect(page.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeVisible();
    expect(entranceTime(await sampledFrames(page))).toBeLessThanOrEqual(ENTRANCE_BUDGET_MS);

    await startSampling(page, '[role="dialog"]');
    await page.keyboard.press("Escape");
    expect(exitTime(await sampledFrames(page))).toBeLessThanOrEqual(EXIT_BUDGET_MS);
  });

  test("dialog scrims don't blur what's behind them", async ({ page }) => {
    await page.keyboard.press("Control+/");
    await expect(page.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeVisible();
    const filters = await page.evaluate(() =>
      [...document.querySelectorAll("body > *")].map((element) => getComputedStyle(element).backdropFilter),
    );
    expect(filters.every((filter) => filter === "none" || filter === "")).toBe(true);
  });
});

test("with reduced motion, a menu fades without growing", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openApp(page);
  await startSampling(page, '[role="menu"]');
  await page.getByRole("button", { name: /^Ada\b/ }).click();
  await expect(page.getByRole("menu")).toBeVisible();
  const frames = await sampledFrames(page);
  expect(frames.filter((frame) => frame.isPresent).every((frame) => IDENTITY.includes(frame.transform))).toBe(true);
});

/** Whether `highlight` sits exactly on `target`, with no move in flight. */
async function isHighlightOn(page: Page, highlightSelector: string, targetSelector: string): Promise<boolean> {
  return page.evaluate(
    ({ highlightSelector, targetSelector }) => {
      const highlight = document.querySelector(highlightSelector)?.getBoundingClientRect();
      const target = document.querySelector(targetSelector)?.getBoundingClientRect();
      if (!highlight || !target) throw new Error("no highlight or target");
      return Math.abs(highlight.top - target.top) < 0.5 && Math.abs(highlight.left - target.left) < 0.5;
    },
    { highlightSelector, targetSelector },
  );
}

test.describe("selection highlights", () => {
  test.beforeEach(async ({ page }) => openApp(page));

  test("arrow keys move the palette's highlight at once", async ({ page }) => {
    await page.keyboard.press("Control+k");
    await expect(page.getByRole("option").first()).toHaveAttribute("aria-selected", "true");

    await pressAndWaitFrames(page, { key: "ArrowDown" });
    expect(await isHighlightOn(page, '[aria-selected="true"] > div.absolute', '[aria-selected="true"]')).toBe(true);
  });

  test("arrow keys move the settings section's highlight at once", async ({ page }) => {
    await page.keyboard.press("Control+,");
    const settings = page.getByRole("dialog", { name: "Settings" });
    await expect(settings.getByRole("tab", { name: "Profile" })).toBeFocused();

    await pressAndWaitFrames(page, { key: "ArrowDown" });
    await expect(settings.getByRole("tab", { name: "Appearance" })).toHaveAttribute("aria-selected", "true");
    expect(
      await isHighlightOn(page, '[role="tab"][aria-selected="true"] > .absolute', '[role="tab"][aria-selected="true"]'),
    ).toBe(true);
  });

  test("a click slides the settings section's highlight briefly", async ({ page }) => {
    await page.keyboard.press("Control+,");
    const settings = page.getByRole("dialog", { name: "Settings" });
    await expect(settings.getByRole("tab", { name: "Profile" })).toBeFocused();
    await startSampling(page, '[role="tab"][aria-selected="true"] > .absolute');

    await settings.getByRole("tab", { name: "Notifications" }).click();
    const frames = (await sampledFrames(page)).filter((frame) => frame.isPresent);
    const started = frames.findIndex((frame) => !IDENTITY.includes(frame.transform));
    expect(started).toBeGreaterThan(-1);
    // Within two pixels of where it ends is arrived: a spring's last steps are too small to see.
    const final = frames[frames.length - 1];
    const arrived = frames.findIndex((frame, index) => index > started && Math.abs(frame.top - final.top) < 2);
    expect(timeBetween(frames, started, arrived)).toBeLessThanOrEqual(ENTRANCE_BUDGET_MS);
  });

  test("alt and the arrows move the sidebar's highlight at once", async ({ page }) => {
    await page.getByRole("button", { name: "New channel" }).click();
    await page.getByLabel("Channel name").fill("random");
    await page.getByLabel("Channel name").press("Enter");
    await expect(page.getByLabel("Message #random")).toBeVisible();
    await page.getByRole("button", { name: "general", exact: true }).click();
    await expect(page.getByLabel("Message #general")).toBeVisible();

    await pressAndWaitFrames(page, { key: "ArrowDown", altKey: true });
    await expect(page.getByRole("button", { name: "random", exact: true })).toHaveAttribute("aria-current", "page");
    const pill = '[data-sidebar] div:has(> [aria-current="page"]) > div.absolute';
    expect(await isHighlightOn(page, pill, '[data-sidebar] div:has(> [aria-current="page"])')).toBe(true);
  });
});
