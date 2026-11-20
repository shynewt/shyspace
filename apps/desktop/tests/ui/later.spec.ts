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

function later(page: Page) {
  return page.getByRole("button", { name: "Later", exact: true });
}

test("a saved message waits in Later until done, and done can be undone", async ({ page }) => {
  await messageMenu(page, "Read the spec");
  await page.getByRole("menuitem", { name: "Save for later" }).click();

  await later(page).click();
  const list = page.getByRole("list", { name: "Saved for later" });
  await expect(list).toContainText("Read the spec");
  await list.getByRole("listitem").hover();
  await list.getByRole("button", { name: "Mark as done" }).click();

  await expect(list.getByRole("listitem")).toHaveCount(0);
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(list).toContainText("Read the spec");

  await list.getByRole("button", { name: "Mark as done" }).click();
  await page.getByRole("button", { name: /Completed/ }).click();
  await expect(page.getByRole("list", { name: "Completed" })).toContainText("Read the spec");
});

test("a saved item has its actions on right-click", async ({ page }) => {
  await messageMenu(page, "Read the spec");
  await page.getByRole("menuitem", { name: "Save for later" }).click();
  await later(page).click();

  await page.getByRole("list", { name: "Saved for later" }).getByRole("listitem").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Remove from Later" }).click();
  await expect(page.getByText("Nothing saved for later")).toBeVisible();
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(page.getByRole("list", { name: "Saved for later" })).toContainText("Read the spec");
});

test("a reminder fires when it comes due", async ({ page }) => {
  await messageMenu(page, "Read the spec");
  await chooseInSubmenu(page, "Remind me", "In 20 minutes");
  await later(page).click();
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

test("Later stands out only when a reminder is due", async ({ page }) => {
  await messageMenu(page, "Read the spec");
  await chooseInSubmenu(page, "Remind me", "In 20 minutes");
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(later(page)).toHaveCSS("font-weight", "400");

  await page.evaluate(() =>
    window.fake.handle("save_for_later", {
      item: window.fake.messageWithText("Read the spec"),
      remindMs: Date.now() + 1000,
    }),
  );
  const due = page.getByRole("button", { name: "Later, 1 due" });
  await expect(due).toBeVisible({ timeout: 10_000 });
  await expect(due).toHaveCSS("font-weight", "600");

  await due.click();
  await page.getByRole("list", { name: "Saved for later" }).getByRole("button", { name: "Mark as done" }).click();
  await expect(later(page)).toBeVisible();
});

test("a page can be saved from its header", async ({ page }) => {
  await page.getByRole("button", { name: "New page" }).click();
  await page.getByLabel("Page title").fill("Spec");
  await page.getByLabel("Page title").press("Enter");
  await page.getByRole("button", { name: "Save for later" }).click();

  await expect(page.getByRole("button", { name: "Saved", exact: true })).toHaveAttribute("aria-pressed", "true");
  await later(page).click();
  await expect(page.getByRole("list", { name: "Saved for later" })).toContainText("Spec");
});

test("a saved message shows a mark in chat that leads to Later", async ({ page }) => {
  const message = page.getByRole("article").filter({ hasText: "Read the spec" });
  await expect(message.getByRole("button", { name: /Saved for later/ })).toHaveCount(0);

  await messageMenu(page, "Read the spec");
  await page.getByRole("menuitem", { name: "Save for later" }).click();
  await expect(message.getByRole("button", { name: "Saved for later. Open Later" })).toBeVisible();

  await messageMenu(page, "Read the spec");
  await chooseInSubmenu(page, "Remind me", "In 20 minutes");
  const reminder = message.getByRole("button", { name: /^Reminder (Today|Tomorrow) .*\. Open Later$/ });
  await expect(reminder).toBeVisible();

  await reminder.click();
  await expect(page.getByRole("heading", { name: "Later" })).toBeVisible();
  await page.getByRole("list", { name: "Saved for later" }).getByRole("button", { name: "Mark as done" }).click();
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(message.getByRole("button", { name: /Open Later/ })).toHaveCount(0);
});

test("a saved message that links to another shows the link as words", async ({ page }) => {
  const linking = await page.evaluate(() =>
    window.fake.receiveMessage(
      window.fake.addMember("Hopper"),
      "general",
      `see shy://message/${window.fake.messageWithText("Read the spec")} first`,
    ),
  );
  expect(linking).toBeTruthy();
  const article = page.getByRole("article").filter({ hasText: "first" }).first();
  await article.hover();
  await article.getByRole("button", { name: "More actions" }).first().click();
  await page.getByRole("menuitem", { name: "Save for later" }).click();

  await later(page).click();
  const list = page.getByRole("list", { name: "Saved for later" });
  await expect(list).toContainText("see a linked message first");
  await expect(list).not.toContainText("shy://");
});
