import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  for (const name of ["design", "random"]) {
    await page.getByRole("button", { name: "New channel" }).click();
    await page.getByLabel("Channel name").fill(name);
    await page.getByLabel("Channel name").press("Enter");
    await expect(page.getByLabel(`Message #${name}`)).toBeVisible();
    await expect(page.getByLabel("Channel name")).toHaveCount(0);
  }
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("alt and the arrows move through the sidebar", async ({ page }) => {
  // Channels sort A to Z: design, general, random.
  await page.keyboard.press("Alt+ArrowDown");
  await expect(page.getByLabel("Message #random")).toBeVisible();
  await page.keyboard.press("Alt+ArrowUp");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Alt+ArrowUp");
  await expect(page.getByLabel("Message #design")).toBeVisible();
});

test("alt shift and the arrows jump to the next unread channel", async ({ page }) => {
  await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "random", "news"));
  await expect(page.getByRole("button", { name: "random, 1 unread" })).toBeVisible();

  await page.keyboard.press("Alt+Shift+ArrowDown");
  await expect(page.getByLabel("Message #random")).toBeVisible();
});

test("shortcuts open the inbox and the cheat sheet", async ({ page }) => {
  await page.keyboard.press("Control+Shift+A");
  await expect(page.getByRole("heading", { name: "Inbox" })).toBeVisible();

  await page.keyboard.press("Control+/");
  const sheet = page.getByRole("dialog", { name: "Keyboard shortcuts" });
  await expect(sheet).toContainText("Next unread channel");
});
