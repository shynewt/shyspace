import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
  const composer = page.getByLabel("Message #general");
  await composer.fill("Alpha");
  await composer.press("Enter");
});

test("using a menu with the mouse leaves no focus ring behind", async ({ page }) => {
  const message = page.getByRole("article").filter({ hasText: "Alpha" });
  await message.hover();
  const trigger = message.getByRole("button", { name: "More actions" });
  await trigger.click();
  await page.getByRole("menuitem", { name: "Save for later" }).click();

  await expect(trigger).toBeFocused();
  await expect(trigger).toHaveCSS("outline-style", "none");
});

test("moving with the keyboard shows where focus is", async ({ page }) => {
  await page.getByRole("button", { name: "Inbox", exact: true }).click();
  await page.keyboard.press("Tab");

  await expect(page.locator(":focus")).toHaveCSS("outline-style", "solid");
});
