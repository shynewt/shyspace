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

test("the header shows the page's path, and each part opens", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    window.fake.seedPage("TEST", "");
    window.fake.seedPage("Test2", "", "TEST");
  });
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Test2", exact: true }).click();
  const path = page.getByRole("navigation", { name: "Breadcrumb" });
  await expect(path.getByRole("listitem")).toHaveText(["TEST", "Test2"]);

  await path.getByRole("button", { name: "Test2", exact: true }).click();
  await expect(page.getByLabel("Page title")).toBeFocused();

  await path.getByRole("button", { name: "TEST", exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue("TEST");
  await expect(path.getByRole("listitem")).toHaveText(["TEST"]);
});

test("saving a page for later is a bookmark toggle", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => window.fake.seedPage("Spec", ""));
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Spec", exact: true }).click();

  await page.getByRole("button", { name: "Save for later" }).click();
  const saved = page.getByRole("button", { name: "Saved for later" });
  await expect(saved).toHaveAttribute("aria-pressed", "true");
  await saved.click();
  await expect(page.getByRole("button", { name: "Save for later" })).toHaveAttribute("aria-pressed", "false");
});

test("rare page actions wait in the more menu", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => window.fake.seedPage("Spec", ""));
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Spec", exact: true }).click();
  await expect(page.getByRole("button", { name: "Delete", exact: true })).toHaveCount(0);

  await page.getByRole("button", { name: "More page actions" }).click();
  await expect(page.getByRole("menuitem")).toHaveText(["New sub-page", "Delete page"]);
});
