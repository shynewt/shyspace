/** On macOS the window's buttons sit on a strip the app draws, in its own colours. */

import { expect, test } from "@playwright/test";

const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";

test.describe("on macOS", () => {
  test.use({ userAgent: MAC });

  test.beforeEach(async ({ page }) => {
    await page.goto("");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("a strip above the app holds the window buttons and moves the window", async ({ page }) => {
    const strip = page.locator("[data-tauri-drag-region]");
    await expect(strip).toBeVisible();
    const stripBox = await strip.boundingBox();
    const sidebarBox = await page.locator("[data-sidebar]").boundingBox();
    expect(stripBox?.y).toBe(0);
    expect(sidebarBox?.y).toBe(stripBox?.height);
  });

  test("full screen hides the strip, as macOS hides the buttons there", async ({ page }) => {
    const strip = page.locator("[data-tauri-drag-region]");
    await page.evaluate(() => window.fake.setFullscreen(true));
    await expect(strip).toHaveCount(0);
    await expect.poll(async () => (await page.locator("[data-sidebar]").boundingBox())?.y).toBe(0);

    await page.evaluate(() => window.fake.setFullscreen(false));
    await expect(strip).toBeVisible();
  });

  test("a window that opens in full screen starts without the strip", async ({ page }) => {
    await page.addInitScript(() => {
      window.prepareFake = (fake) => fake.setFullscreen(true);
    });
    await page.reload();
    await expect(page.getByLabel("Message #general")).toBeVisible();
    await expect(page.locator("[data-tauri-drag-region]")).toHaveCount(0);
  });
});

test("Windows and Linux keep the system's own title bar", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect(page.locator("[data-tauri-drag-region]")).toHaveCount(0);
});
