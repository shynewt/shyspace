import { expect, test } from "@playwright/test";

test("a new page can be titled and written in", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();

  const title = page.getByLabel("Page title");
  await title.fill("Roadmap");
  await title.press("Enter");
  await expect(page.getByRole("navigation", { name: "Workspace" }).getByText("Roadmap")).toBeVisible();

  await page.locator(".cm-content").click();
  await page.keyboard.type("# Q1\nShip the desktop app");

  await expect.poll(() => page.evaluate(() => window.fake.pageText("Roadmap"))).toBe("# Q1\nShip the desktop app");
});

test("an open page reloads when its content is rebuilt", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();
  const title = page.getByLabel("Page title");
  await title.fill("Notes");
  await title.press("Enter");
  await page.locator(".cm-content").click();
  await page.keyboard.type("draft from a removed member");
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Notes"))).toBe("draft from a removed member");

  await page.evaluate(() => window.fake.rebuildPage("Notes", "what everyone agrees on"));

  await expect(page.locator(".cm-content")).toHaveText("what everyone agrees on");
});
