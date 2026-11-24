import { expect, type Page, test } from "@playwright/test";

const lines = (page: Page) => page.locator(".cm-line");
const caret = (page: Page) => page.locator(".cm-remote-caret");

/** Opens a page "Plan" with `text`; Grace is a member. Returns the page's and Grace's ids. */
/** `withFakeClock` lets a test move time on; it also holds animation frames, so only time-travelling tests use it. */
async function openPlan(
  page: Page,
  text: string,
  { preferences, withFakeClock = false }: { preferences?: Record<string, unknown>; withFakeClock?: boolean } = {},
) {
  if (withFakeClock) await page.clock.install();
  if (preferences) {
    await page.addInitScript(
      (stored) => localStorage.setItem("shyspace.preferences", JSON.stringify(stored)),
      preferences,
    );
  }
  await page.goto("");
  const ids = await page.evaluate((body) => {
    const pageId = window.fake.seedPage("Plan", body);
    const grace = window.fake.addMember("Grace");
    return { pageId, grace };
  }, text);
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Plan", exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue("Plan");
  return ids;
}

async function graceAt(page: Page, ids: { pageId: string; grace: string }, anchor: number, head = anchor) {
  await page.evaluate(
    ({ pageId, grace, anchor, head }) => {
      window.fake.pageCursors.arrive(grace, pageId);
      window.fake.pageCursors.moveCursor(grace, pageId, anchor, head);
    },
    { ...ids, anchor, head },
  );
}

test("another member's cursor shows, named for a moment after it moves", async ({ page }) => {
  const ids = await openPlan(page, "Ship it on Friday", { withFakeClock: true });
  await graceAt(page, ids, 5);

  await expect(caret(page)).toHaveCount(1);
  const label = page.locator(".cm-remote-label", { hasText: "Grace" });
  await expect(label).toBeVisible();
  await page.clock.fastForward(2500);
  await expect(label).toBeHidden();
});

test("a cursor's name shows on hover", async ({ page }) => {
  // On the real clock: the fake one holds back the frames that hover state waits for.
  const ids = await openPlan(page, "Ship it on Friday");
  await graceAt(page, ids, 5);
  const label = page.locator(".cm-remote-label", { hasText: "Grace" });
  await expect(label).toBeHidden({ timeout: 5000 });
  // The caret is all border, so hover its middle rather than its (empty) content box.
  const box = await caret(page).boundingBox();
  if (!box) throw new Error("the caret isn't on screen");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await expect(label).toBeVisible();
});

test("another member's selection is tinted, and their cursor goes when they leave", async ({ page }) => {
  const ids = await openPlan(page, "Ship it on Friday");
  await graceAt(page, ids, 0, 4);
  await expect(page.locator(".cm-remote-selection")).toHaveText("Ship");

  await page.evaluate(({ pageId, grace }) => window.fake.pageCursors.leave(grace, pageId), ids);
  await expect(caret(page)).toHaveCount(0);
  await expect(page.locator(".cm-remote-selection")).toHaveCount(0);
});

test("a cursor fades after a minute without moving", async ({ page }) => {
  const ids = await openPlan(page, "Ship it on Friday", { withFakeClock: true });
  await graceAt(page, ids, 5);
  await expect(caret(page)).not.toHaveClass(/is-idle/);
  await page.clock.fastForward(61_000);
  await expect(caret(page)).toHaveClass(/is-idle/);
});

test("my cursor is shared while I'm in the text", async ({ page }) => {
  const { pageId } = await openPlan(page, "Ship it on Friday");
  await expect.poll(() => page.evaluate((id) => window.fake.pageCursors.joined.get(id), pageId)).toBe(true);
  await lines(page).first().click();
  await expect
    .poll(() =>
      page.evaluate(
        (id) => window.fake.pageCursors.sent.filter((sent) => sent.page === id && sent.selection).length,
        pageId,
      ),
    )
    .toBeGreaterThan(0);
});

test("with sharing off nothing is sent, and others still show", async ({ page }) => {
  const ids = await openPlan(page, "Ship it on Friday", { preferences: { shareCursorInPages: false } });
  await expect.poll(() => page.evaluate((id) => window.fake.pageCursors.joined.get(id), ids.pageId)).toBe(false);
  await lines(page).first().click();
  await page.keyboard.press("End");
  await graceAt(page, ids, 5);
  await expect(caret(page)).toHaveCount(1);
  expect(await page.evaluate(() => window.fake.pageCursors.sent.length)).toBe(0);
});

test("turning sharing off with a page open switches it there, without leaving the page", async ({ page }) => {
  const ids = await openPlan(page, "Ship it on Friday");
  await expect.poll(() => page.evaluate((id) => window.fake.pageCursors.joined.get(id), ids.pageId)).toBe(true);

  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Privacy" }).click();
  await settings
    .getByRole("radiogroup", { name: "Your cursor in pages" })
    .getByRole("radio", { name: "Don't share" })
    .click();

  await expect.poll(() => page.evaluate((id) => window.fake.pageCursors.joined.get(id), ids.pageId)).toBe(false);
  expect(await page.evaluate(() => window.fake.pageCursors.left)).toEqual([]);
});

test("others' cursors can be hidden", async ({ page }) => {
  const ids = await openPlan(page, "Ship it on Friday", { preferences: { showCursorsInPages: false } });
  await graceAt(page, ids, 5);
  await expect(page.getByRole("button", { name: "Go to Grace's cursor" })).toBeVisible();
  await expect(caret(page)).toHaveCount(0);
});

test("the header shows who is here, and one click goes to their cursor", async ({ page }) => {
  const text = Array.from({ length: 120 }, (_, index) => `Line ${index + 1}`).join("\n");
  const ids = await openPlan(page, text);
  const lineHundred = text.indexOf("Line 100");
  await graceAt(page, ids, lineHundred);
  // The page opens at its top, far from line 100.
  await page.getByRole("button", { name: "Go to Grace's cursor" }).click();
  await expect(lines(page).filter({ hasText: /Line 100(?!\d)/ })).toBeInViewport();
});
