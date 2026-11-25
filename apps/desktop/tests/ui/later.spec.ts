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
  // A reminder isn't answered from the notification: no Reply or Mark as read.
  expect(await page.evaluate(() => window.fake.alertKinds)).toEqual(["reminder"]);
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

  await expect(page.getByRole("button", { name: "Saved for later" })).toHaveAttribute("aria-pressed", "true");
  await later(page).click();
  await expect(page.getByRole("list", { name: "Saved for later" })).toContainText("Spec");
});

test("a saved message is labelled above its text, also when grouped, until done", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.receiveMessage(window.fake.memberNamed("Grace"), "general", "And the appendix"),
  );
  const messages = [
    { article: page.getByRole("article").filter({ hasText: "Read the spec" }), text: "Read the spec" },
    { article: page.getByRole("article").filter({ hasText: "And the appendix" }), text: "And the appendix" },
  ];
  for (const { article } of messages) {
    await expect(article.getByRole("button", { name: /Saved for later|Reminder/ })).toHaveCount(0);
    await article.hover();
    await article.getByRole("button", { name: "More actions" }).click();
    await page.getByRole("menuitem", { name: "Save for later" }).click();
  }
  for (const { article, text } of messages) {
    const label = await article.getByRole("button", { name: "Saved for later. Open Later", exact: true }).boundingBox();
    const body = await article.getByText(text, { exact: true }).boundingBox();
    if (!label || !body) throw new Error("message not laid out");
    expect(label.y + label.height).toBeLessThanOrEqual(body.y + 1);
    await expect(article).toHaveAccessibleName(/saved for later/i);
  }

  const first = messages[0].article;
  await first.hover();
  await first.getByRole("button", { name: "More actions" }).click();
  await chooseInSubmenu(page, "Remind me", "In 20 minutes");
  const reminder = first.getByRole("button", { name: /^Reminder · (Today|Tomorrow) .*\. Open Later$/ });
  await expect(reminder).toBeVisible();
  await expect(first).toHaveAccessibleName(/reminder · (today|tomorrow)/i);

  await reminder.click();
  await expect(page.getByRole("heading", { name: "Later" })).toBeVisible();
  await page
    .getByRole("list", { name: "Saved for later" })
    .getByRole("listitem")
    .filter({ hasText: "Read the spec" })
    .getByRole("button", { name: "Mark as done" })
    .click();
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(first.getByRole("button", { name: /Reminder/ })).toHaveCount(0);
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

test("a reminder's toast opens what you saved", async ({ page }) => {
  await messageMenu(page, "Read the spec");
  await chooseInSubmenu(page, "Remind me", "In 20 minutes");
  await later(page).click();
  await page.evaluate(() =>
    window.fake.handle("save_for_later", {
      item: window.fake.messageWithText("Read the spec"),
      remindMs: Date.now() + 1000,
    }),
  );

  const toast = page.getByRole("status").filter({ hasText: "Reminder: Read the spec" });
  await toast.getByRole("button", { name: "Open" }).click({ timeout: 10_000 });
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect(toast).toHaveCount(0);
});

test("Later keeps a reminder beside its item, in a readable width", async ({ page }) => {
  await messageMenu(page, "Read the spec");
  await chooseInSubmenu(page, "Remind me", "In 20 minutes");
  await later(page).click();

  const list = page.getByRole("list", { name: "Saved for later" });
  const where = await list.getByText("Grace in general").boundingBox();
  const reminder = await list.getByText(/(Today|Tomorrow) \d/).boundingBox();
  if (!where || !reminder) throw new Error("Later not laid out");
  expect(reminder.x - (where.x + where.width)).toBeLessThan(24);
  expect((await list.boundingBox())?.width).toBeLessThanOrEqual(760);
});
