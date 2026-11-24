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

test("text typed before the saved drafts load is still kept as a draft", async ({ page }) => {
  await page.getByRole("button", { name: "general", exact: true }).click();
  await page.evaluate(() => {
    window.fake.holdDrafts = true;
  });
  await page.getByRole("button", { name: "random", exact: true }).click();
  await page.getByLabel("Message #random").fill("typed quickly");
  await page.evaluate(() => {
    window.fake.holdDrafts = false;
    window.fake.finishDrafts();
  });

  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(page.getByRole("button", { name: "random, draft" })).toBeVisible();
});

test("drafts wait in Drafts & sent, not in the inbox, and sending clears them", async ({ page }) => {
  await page.getByLabel("Message #random").fill("finish this later");
  await page.keyboard.press("Control+Shift+A");
  await expect(page.getByRole("heading", { name: "Inbox" })).toBeVisible();
  await expect(page.getByText("You're all caught up")).toBeVisible();
  await expect(page.getByText("finish this later")).toHaveCount(0);

  await page.getByRole("button", { name: "Drafts & sent, 1 draft" }).click();
  await page
    .getByRole("list", { name: "Drafts" })
    .getByRole("button", { name: /finish this later/ })
    .click();
  const composer = page.getByLabel("Message #random");
  await expect(composer).toHaveValue("finish this later");
  await composer.press("Enter");
  await expect(page.getByRole("button", { name: "random", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Drafts & sent", exact: true })).toBeVisible();
});
