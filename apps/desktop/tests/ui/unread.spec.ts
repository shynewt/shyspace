import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

async function receiveInRandom(page: import("@playwright/test").Page, ...texts: string[]) {
  await page.evaluate((texts) => {
    const grace = window.fake.addMember("Grace");
    for (const text of texts) window.fake.receiveMessage(grace, "random", text);
  }, texts);
}

test("messages in another channel show as unread until you open it", async ({ page }) => {
  await receiveInRandom(page, "Coffee?", "Anyone?");

  const random = page.getByRole("button", { name: "random, 2 unread" });
  await expect(random).toBeVisible();
  await random.click();

  await expect(page.getByLabel("Message #random")).toBeVisible();
  await expect(page.getByRole("button", { name: "random", exact: true })).toBeVisible();
});

test("a new-messages line marks where you left off until Escape", async ({ page }) => {
  await page.getByRole("button", { name: "random", exact: true }).click();
  await page.getByLabel("Message #random").fill("Morning all");
  await page.getByLabel("Message #random").press("Enter");
  await page.getByRole("button", { name: "general", exact: true }).click();
  await receiveInRandom(page, "Coffee?");

  await page.getByRole("button", { name: "random, 1 unread" }).click();
  const line = page.getByRole("separator", { name: "New messages" });
  await expect(line).toBeVisible();
  const articles = page.getByRole("article");
  await expect(articles.first()).toContainText("Morning all");

  await page.keyboard.press("Escape");
  await expect(line).toHaveCount(0);
});

test("the unread filter hides channels you've read", async ({ page }) => {
  const sidebar = page.getByRole("navigation", { name: "Workspace" });
  await page.getByRole("button", { name: "Show unread channels only" }).click();
  await expect(sidebar.getByRole("button", { name: "random", exact: true })).toHaveCount(0);
  await expect(sidebar.getByRole("button", { name: "general", exact: true })).toBeVisible();

  await receiveInRandom(page, "Coffee?");
  await expect(sidebar.getByRole("button", { name: "random, 1 unread" })).toBeVisible();

  await page.getByRole("button", { name: "Show all channels" }).click();
});
