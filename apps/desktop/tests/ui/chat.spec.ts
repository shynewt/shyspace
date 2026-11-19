import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
});

test("sending a message shows it once and clears the composer", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Hello team");
  await composer.press("Enter");

  await expect(page.getByRole("article").getByText("Hello team")).toHaveCount(1);
  await expect(composer).toHaveValue("");
});

test("shift enter adds a line instead of sending", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("first line");
  await composer.press("Shift+Enter");
  await composer.pressSequentially("second line");

  await expect(composer).toHaveValue("first line\nsecond line");
  await expect(page.getByRole("article")).toHaveCount(0);
});

test("a new channel appears once and opens", async ({ page }) => {
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("design");
  await page.getByLabel("Channel name").press("Enter");

  await expect(page.getByRole("button", { name: "design", exact: true })).toHaveCount(1);
  await expect(page.getByLabel("Message #design")).toBeVisible();
});

test("messages from other members appear live", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", "Morning! **Bold** plan");
  });

  await expect(page.getByText("Grace")).toBeVisible();
  await expect(page.locator("strong", { hasText: "Bold" })).toBeVisible();
});

test("reacting adds a count that a second click removes", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", "Shipped it");
  });
  const message = page.getByText("Shipped it");
  await message.hover();
  await page.getByRole("button", { name: "React" }).click();
  await page.getByRole("button", { name: "React with 👍" }).click();

  const reaction = page.getByRole("button", { name: "👍 1" });
  await expect(reaction).toBeVisible();
  await reaction.click();
  await expect(reaction).toHaveCount(0);
});

test("untrusted markdown never becomes live HTML", async ({ page }) => {
  await page.evaluate(() => {
    const mallory = window.fake.addMember("Mallory");
    window.fake.receiveMessage(mallory, "general", '<img src=x onerror="window.pwned=1"> <b>not bold</b>');
  });

  await expect(page.getByText("Mallory")).toBeVisible();
  await expect(page.locator("main img")).toHaveCount(0);
  expect(await page.evaluate(() => "pwned" in window)).toBe(false);
});
