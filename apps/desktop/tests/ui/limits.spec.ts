import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a message near the limit shows a count, and one over it can't be sent", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("a".repeat(30_000));
  await expect(page.getByText(/\/ 40,000/)).toHaveCount(0);

  await composer.fill("a".repeat(39_000));
  await expect(page.getByText("39,000 / 40,000")).toBeVisible();
  await expect(page.getByRole("button", { name: "Send" })).toBeEnabled();

  await composer.fill("a".repeat(40_001));
  await expect(page.getByText("Too long to send: 40,001 / 40,000")).toBeVisible();
  await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();
  await composer.press("Enter");
  await expect(composer).toHaveValue("a".repeat(40_001));
});

test("a page title stops at 200 characters", async ({ page }) => {
  await page.getByRole("button", { name: "New page" }).click();
  await page.getByLabel("Page title").fill("t".repeat(250));
  await expect(page.getByLabel("Page title")).toHaveValue("t".repeat(200));

  // Counted as the core counts: an emoji is one character, not two.
  await page.getByLabel("Page title").fill("🚀".repeat(150));
  await expect(page.getByLabel("Page title")).toHaveValue("🚀".repeat(150));
});
