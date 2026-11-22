import { expect, test } from "@playwright/test";

test.describe("the sidebar", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("is docked in a wide window", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    const nav = page.getByRole("navigation", { name: "Workspace" });
    await expect(nav).toBeVisible();
    await expect(page.getByRole("button", { name: "Show sidebar" })).toHaveCount(0);
    await expect(page.getByRole("dialog", { name: "Sidebar" })).toHaveCount(0);
    // The shell fades and scales in as the app opens; measure once it's settled.
    const sidebar = page.getByRole("complementary", { name: "Sidebar" });
    await expect.poll(async () => (await sidebar.boundingBox())?.x).toBe(0);
    expect((await sidebar.boundingBox())?.width).toBe(264);
    await expect.poll(async () => (await page.getByRole("main").boundingBox())?.x).toBe(264);
  });

  test("is narrower between 760 and 1100 px", async ({ page }) => {
    await page.setViewportSize({ width: 1000, height: 800 });
    const sidebar = page.getByRole("complementary", { name: "Sidebar" });
    await expect.poll(async () => (await sidebar.boundingBox())?.width).toBe(232);
  });

  test("stays docked in half of a laptop screen", async ({ page }) => {
    await page.setViewportSize({ width: 800, height: 800 });
    await expect(page.getByRole("navigation", { name: "Workspace" })).toBeVisible();
    await expect(page.getByRole("dialog", { name: "Sidebar" })).toHaveCount(0);
    await expect.poll(async () => (await page.getByRole("main").boundingBox())?.x).toBe(232);
  });

  test("is a drawer in a narrow window: Esc closes it and focus returns", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 800 });
    await expect(page.getByRole("navigation", { name: "Workspace" })).toHaveCount(0);
    await expect.poll(async () => (await page.getByRole("main").boundingBox())?.x).toBe(0);
    const show = page.getByRole("button", { name: "Show sidebar" });
    await show.click();

    const drawer = page.getByRole("dialog", { name: "Sidebar" });
    await expect(drawer.getByRole("navigation", { name: "Workspace" })).toBeVisible();
    await expect(drawer).toBeInViewport({ ratio: 1 });
    await expect.poll(() => drawer.evaluate((element) => element.contains(document.activeElement))).toBe(true);
    await page.keyboard.press("Escape");
    await expect(drawer).toBeHidden();
    await expect(show).toBeFocused();
  });

  test("closes on a click beside it", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 800 });
    await page.getByRole("button", { name: "Show sidebar" }).click();
    const drawer = page.getByRole("dialog", { name: "Sidebar" });
    await expect(drawer).toBeVisible();
    await page.mouse.click(680, 400);
    await expect(drawer).toBeHidden();
  });

  test("closes once you choose where to go", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 800 });
    await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
    await page.getByRole("button", { name: "Show sidebar" }).click();
    const drawer = page.getByRole("dialog", { name: "Sidebar" });
    await drawer.getByRole("button", { name: "random", exact: true }).click();

    await expect(drawer).toHaveCount(0);
    // Radix hands focus back in a task queued as the drawer goes; let it run, then focus must still be in the channel.
    await page.evaluate(() => new Promise((resolve) => setTimeout(resolve, 0)));
    await expect(page.getByLabel("Message #random")).toBeFocused();

    await page.getByRole("button", { name: "Show sidebar" }).click();
    await drawer.getByRole("button", { name: /^Inbox/ }).click();
    await expect(drawer).toBeHidden();
    await expect(page.getByRole("heading", { name: "Inbox" })).toBeVisible();
  });

  test("closes when you choose where you already are", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 800 });
    await page.getByRole("button", { name: "Show sidebar" }).click();
    const drawer = page.getByRole("dialog", { name: "Sidebar" });
    await drawer.getByRole("button", { name: "general", exact: true }).click();
    await expect(drawer).toHaveCount(0);
    await expect(page.getByRole("main")).toBeFocused();
  });

  test("can still be shown with no channel to show", async ({ page }) => {
    await page.getByRole("button", { name: "Hide sidebar" }).click();
    await page.evaluate(() => window.fake.handle("delete_channel", { channel: window.fake.channelId("general") }));
    await expect(page.getByLabel("Message #general")).toHaveCount(0);
    await page.getByRole("button", { name: "Show sidebar" }).click();
    await expect(page.getByRole("complementary", { name: "Sidebar" })).toBeVisible();
  });

  test("an open drawer that docks as the window grows leaves focus in the content", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 800 });
    await page.getByRole("button", { name: "Show sidebar" }).click();
    await expect(page.getByRole("dialog", { name: "Sidebar" })).toBeVisible();
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect(page.getByRole("dialog", { name: "Sidebar" })).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => document.activeElement?.tagName)).toBe("MAIN");
  });

  test("the shortcut opens and closes the drawer", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 800 });
    await expect(page.getByRole("button", { name: "Show sidebar" })).toBeVisible();
    const drawer = page.getByRole("dialog", { name: "Sidebar" });
    await page.keyboard.press("Control+\\");
    await expect(drawer).toBeVisible();
    await page.keyboard.press("Control+\\");
    await expect(drawer).toBeHidden();
  });

  test("can be hidden in a wide window, and stays hidden after a restart", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    const sidebar = page.getByRole("complementary", { name: "Sidebar" });
    await page.getByRole("button", { name: "Hide sidebar" }).click();
    await expect(sidebar).toHaveCount(0);
    const show = page.getByRole("button", { name: "Show sidebar" });
    await expect(show).toBeFocused();
    await expect.poll(async () => (await page.getByRole("main").boundingBox())?.x).toBe(0);

    await page.reload();
    await expect(page.getByLabel("Message #general")).toBeVisible();
    await expect(sidebar).toHaveCount(0);
    await show.click();
    await expect(sidebar).toBeVisible();
    await expect(page.getByRole("button", { name: "Hide sidebar" })).toBeFocused();
  });

  test("the shortcut hides and shows it in a wide window", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    const sidebar = page.getByRole("complementary", { name: "Sidebar" });
    await page.keyboard.press("Control+\\");
    await expect(sidebar).toHaveCount(0);
    await page.keyboard.press("Control+\\");
    await expect(sidebar).toBeVisible();
    await expect(page.getByLabel("Message #general")).toBeFocused();
  });

  test("an open drawer docks when the window grows", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 800 });
    await page.getByRole("button", { name: "Show sidebar" }).click();
    await expect(page.getByRole("dialog", { name: "Sidebar" })).toBeVisible();
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect(page.getByRole("dialog", { name: "Sidebar" })).toHaveCount(0);
    await expect(page.getByRole("complementary", { name: "Sidebar" })).toBeVisible();
  });

  test("the drawer doesn't slide with reduced motion", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.setViewportSize({ width: 720, height: 800 });
    await expect(page.getByRole("button", { name: "Show sidebar" })).toBeVisible();
    const left = await page.evaluate(async () => {
      const show = document.querySelector<HTMLElement>('button[aria-label="Show sidebar"]');
      if (!show) throw new Error("no Show sidebar button");
      show.click();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return document.querySelector('[role="dialog"]')?.getBoundingClientRect().left;
    });
    expect(left).toBe(0);
  });

  test("the cheat sheet lists its shortcut", async ({ page }) => {
    await page.keyboard.press("Control+/");
    const sheet = page.getByRole("dialog", { name: "Keyboard shortcuts" });
    await expect(sheet.getByRole("listitem").filter({ hasText: "Show or hide the sidebar" })).toContainText("\\");
  });
});
