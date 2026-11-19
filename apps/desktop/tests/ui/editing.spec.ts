import { expect, type Page, test } from "@playwright/test";

async function openGeneral(page: Page, start = "") {
  await page.goto(start);
  await page.getByRole("button", { name: "general", exact: true }).click();
}

async function messageActions(page: Page, text: string) {
  await page.getByRole("article").filter({ hasText: text }).hover();
  await page.getByRole("button", { name: "More actions" }).click();
}

test("you can edit your own message in place", async ({ page }) => {
  await openGeneral(page);
  const composer = page.getByLabel("Message #general");
  await composer.fill("Lunch at noon");
  await composer.press("Enter");

  await messageActions(page, "Lunch at noon");
  await page.getByRole("menuitem", { name: "Edit" }).click();
  const editor = page.getByLabel("Edit message");
  await editor.fill("Lunch at one");
  await editor.press("Enter");

  const message = page.getByRole("article").filter({ hasText: "Lunch at one" });
  await expect(message).toContainText("(edited)");
  await expect(page.getByRole("article").filter({ hasText: "noon" })).toHaveCount(0);
});

test("escape leaves a message unchanged", async ({ page }) => {
  await openGeneral(page);
  const composer = page.getByLabel("Message #general");
  await composer.fill("Keep me");
  await composer.press("Enter");

  await messageActions(page, "Keep me");
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await page.getByLabel("Edit message").fill("Changed my mind");
  await page.getByLabel("Edit message").press("Escape");

  await expect(page.getByLabel("Edit message")).toHaveCount(0);
  await expect(page.getByRole("article").filter({ hasText: "Keep me" })).not.toContainText("(edited)");
});

test("the admin can delete someone else's message but not edit it", async ({ page }) => {
  await openGeneral(page);
  await page.evaluate(() => {
    const mallory = window.fake.addMember("Mallory");
    window.fake.receiveMessage(mallory, "general", "Buy cheap watches");
  });

  await messageActions(page, "Buy cheap watches");
  await expect(page.getByRole("menuitem", { name: "Edit" })).toHaveCount(0);
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Delete message" }).click();

  await expect(page.getByRole("article").filter({ hasText: "Buy cheap watches" })).toHaveCount(0);
});

test("a regular member can only copy others' messages", async ({ page }) => {
  await openGeneral(page, "?start=member");
  await page.evaluate(() => window.fake.receiveMessage(window.fake.memberNamed("Ada"), "general", "Welcome!"));

  await messageActions(page, "Welcome!");
  await expect(page.getByRole("menuitem")).toHaveText(["Copy text", "Copy link"]);
});

test("a channel can be renamed from the sidebar", async ({ page }) => {
  await openGeneral(page);
  await page.getByRole("button", { name: "general", exact: true }).hover();
  await page.getByRole("button", { name: "Actions for #general" }).click();
  await page.getByRole("menuitem", { name: "Rename" }).click();
  await page.getByLabel("New name for #general").fill("announcements");
  await page.getByLabel("New name for #general").press("Enter");

  await expect(page.getByRole("button", { name: "announcements", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "general", exact: true })).toHaveCount(0);
});

test("deleting the open channel moves you to another one", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("temp");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #temp")).toBeVisible();

  await page.getByRole("button", { name: "temp", exact: true }).hover();
  await page.getByRole("button", { name: "Actions for #temp" }).click();
  await page.getByRole("menuitem", { name: "Delete channel" }).click();
  await page.getByRole("button", { name: "Delete channel" }).click();

  await expect(page.getByRole("button", { name: "temp", exact: true })).toHaveCount(0);
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("deleting a page keeps its sub-pages at the top level", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();
  await page.getByLabel("Page title").fill("Projects");
  await page.getByLabel("Page title").press("Enter");
  await page.getByRole("button", { name: "Sub-page", exact: true }).click();
  // The sub-page has opened once its breadcrumb leads back to the parent.
  await expect(
    page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("button", { name: "Projects" }),
  ).toBeVisible();
  await page.getByLabel("Page title").fill("Notes");
  await page.getByLabel("Page title").press("Enter");
  const sidebar = page.getByRole("navigation", { name: "Workspace" });
  await sidebar.getByRole("button", { name: "Projects", exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue("Projects");

  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await page.getByRole("button", { name: "Delete page" }).click();

  await expect(sidebar.getByRole("button", { name: "Projects", exact: true })).toHaveCount(0);
  await expect(sidebar.getByRole("button", { name: "Notes", exact: true })).toBeVisible();
  await expect(sidebar.getByRole("button", { name: "Collapse Projects" })).toHaveCount(0);
});
