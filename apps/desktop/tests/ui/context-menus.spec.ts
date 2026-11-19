import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("right-clicking a message opens its menu", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Right-click me");
  await composer.press("Enter");

  await page.getByRole("article").filter({ hasText: "Right-click me" }).click({ button: "right" });
  await expect(page.getByRole("menuitem")).toHaveText(["Copy text", "Copy link", "Edit", "Delete"]);
});

test("right-clicking a page in the sidebar opens its menu", async ({ page }) => {
  await page.getByRole("button", { name: "New page" }).click();
  await page.getByLabel("Page title").fill("Notes");
  await page.getByLabel("Page title").press("Enter");
  const sidebar = page.getByRole("navigation", { name: "Workspace" });

  await sidebar.getByRole("button", { name: "Notes", exact: true }).click({ button: "right" });
  await expect(page.getByRole("menuitem")).toHaveText(["New sub-page", "Copy link", "Delete page"]);
  await page.getByRole("menuitem", { name: "Delete page" }).click();
  await page.getByRole("button", { name: "Delete page" }).click();
  await expect(sidebar.getByRole("button", { name: "Notes", exact: true })).toHaveCount(0);
});
