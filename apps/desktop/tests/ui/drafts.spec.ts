import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #random")).toBeVisible();
});

test("unsent text waits as a draft when you switch away", async ({ page }) => {
  await page.getByLabel("Message #random").fill("half a thought");
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.getByRole("button", { name: "random, draft" }).click();
  await expect(page.getByLabel("Message #random")).toHaveValue("half a thought");
});

test("drafts are listed in the inbox and sending clears them", async ({ page }) => {
  await page.getByLabel("Message #random").fill("finish this later");
  await page.keyboard.press("Control+Shift+A");
  const drafts = page.getByRole("region", { name: "Drafts" });
  await expect(drafts).toContainText("finish this later");

  await drafts.getByRole("button").click();
  const composer = page.getByLabel("Message #random");
  await expect(composer).toHaveValue("finish this later");
  await composer.press("Enter");
  await expect(page.getByRole("button", { name: "random", exact: true })).toBeVisible();
});
