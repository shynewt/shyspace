import { expect, type Page, test } from "@playwright/test";
import { chooseInSubmenu } from "./support";

async function messageMenu(page: Page, text: string) {
  await page.getByRole("article").filter({ hasText: text }).hover();
  await page.getByRole("button", { name: "More actions" }).click();
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "general", "Read the spec"));
  await expect(page.getByRole("article").filter({ hasText: "Read the spec" })).toBeVisible();
});

test("a saved message waits in Later until done", async ({ page }) => {
  await messageMenu(page, "Read the spec");
  await page.getByRole("menuitem", { name: "Save for later" }).click();

  await page.getByRole("button", { name: "Later, 1 saved" }).click();
  const list = page.getByRole("list", { name: "Saved for later" });
  await expect(list).toContainText("Read the spec");
  await list.getByRole("button", { name: "Mark as done" }).click();

  await expect(list.getByRole("listitem")).toHaveCount(0);
  await page.getByRole("button", { name: /Completed/ }).click();
  await expect(page.getByRole("list", { name: "Completed" })).toContainText("Read the spec");
  await expect(page.getByRole("button", { name: "Later", exact: true })).toBeVisible();
});

test("a reminder fires when it comes due", async ({ page }) => {
  await messageMenu(page, "Read the spec");
  await chooseInSubmenu(page, "Remind me", "In 20 minutes");
  await page.getByRole("button", { name: "Later, 1 saved" }).click();
  await expect(page.getByRole("list", { name: "Saved for later" })).toContainText(/Today|Tomorrow/);

  // Move the reminder two seconds ahead, as if twenty minutes had passed.
  await page.evaluate(() =>
    window.fake.handle("save_for_later", {
      item: window.fake.messageWithText("Read the spec"),
      remindMs: Date.now() + 2000,
    }),
  );
  await expect(page.getByText("Reminder: Read the spec")).toBeVisible({ timeout: 10_000 });
  expect(await page.evaluate(() => window.fake.alertsShown)).toEqual([{ title: "Reminder", body: "Read the spec" }]);
});

test("a page can be saved from its header", async ({ page }) => {
  await page.getByRole("button", { name: "New page" }).click();
  await page.getByLabel("Page title").fill("Spec");
  await page.getByLabel("Page title").press("Enter");
  await page.getByRole("button", { name: "Save for later" }).click();

  await expect(page.getByRole("button", { name: "Saved", exact: true })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "Later, 1 saved" }).click();
  await expect(page.getByRole("list", { name: "Saved for later" })).toContainText("Spec");
});
