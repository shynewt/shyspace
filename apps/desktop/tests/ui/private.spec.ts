import { expect, type Page, test } from "@playwright/test";

function workspaceNav(page: Page) {
  return page.getByRole("navigation", { name: "Workspace" });
}

async function openNewChannelMenu(page: Page) {
  const channels = workspaceNav(page).getByRole("region", { name: "Channels" });
  await channels.hover();
  await channels.getByRole("button", { name: "Actions for the Channels section" }).click();
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a private channel is made with chosen people and shows a lock", async ({ page }) => {
  await page.evaluate(() => window.fake.addMember("Grace"));
  await openNewChannelMenu(page);
  await page.getByRole("menuitem", { name: "New private channel" }).click();

  const dialog = page.getByRole("dialog", { name: "New private channel" });
  await dialog.getByLabel("Name").fill("Launch Plans");
  await expect(dialog.getByLabel("Name")).toHaveValue("launch-plans");
  await dialog.getByRole("button", { name: "Add Grace" }).click();
  await dialog.getByRole("button", { name: "Create private channel" }).click();

  await expect(dialog).toBeHidden();
  await expect(page.getByRole("heading", { name: "launch-plans", exact: true })).toBeVisible();
  await expect(page.getByLabel("Private channel")).toBeVisible();
  await expect(workspaceNav(page).getByRole("button", { name: /^Private launch-plans/ })).toBeVisible();
  await expect(page.getByText("Only the people in it can read what you write")).toBeVisible();

  await page.getByRole("button", { name: "People in #launch-plans" }).click();
  const people = page.getByRole("list", { name: "People in this channel" });
  await expect(people).toContainText("Grace");
  await expect(people).toContainText("Owner");
});

test("the owner adds, hands over and removes; others leave", async ({ page }) => {
  await page.evaluate(() => [window.fake.addMember("Grace"), window.fake.addMember("Alan")]);
  await openNewChannelMenu(page);
  await page.getByRole("menuitem", { name: "New private channel" }).click();
  const dialog = page.getByRole("dialog", { name: "New private channel" });
  await dialog.getByLabel("Name").fill("ops");
  await dialog.getByRole("button", { name: "Add Grace" }).click();
  await dialog.getByRole("button", { name: "Create private channel" }).click();

  await page.getByRole("button", { name: "People in #ops" }).click();
  const people = page.getByRole("dialog", { name: "People in #ops" });
  await people.getByRole("button", { name: "Add people" }).click();
  await people.getByRole("button", { name: "Add Alan" }).click();
  await people.getByRole("button", { name: "Done adding" }).click();
  await expect(people.getByRole("list", { name: "People in this channel" })).toContainText("Alan");

  await people.getByRole("button", { name: "Actions for Alan" }).click();
  await page.getByRole("menuitem", { name: "Remove from channel" }).click();
  await page.getByRole("button", { name: "Remove", exact: true }).click();
  await expect(people.getByRole("list", { name: "People in this channel" })).not.toContainText("Alan");

  await expect(people.getByText("To leave, make someone else the owner first.")).toBeVisible();
  await people.getByRole("button", { name: "Actions for Grace" }).click();
  await page.getByRole("menuitem", { name: "Make owner" }).click();
  await people.getByRole("button", { name: "Leave channel" }).click();
  await page.getByRole("dialog", { name: "Leave #ops?" }).getByRole("button", { name: "Leave channel" }).click();

  await expect(page.getByText("You're no longer in this channel.")).toBeVisible();
  await expect(page.getByLabel("Message #ops")).toBeHidden();
});

test("a DM is started from its own section and named after the other person", async ({ page }) => {
  await page.evaluate(() => window.fake.addMember("Grace"));
  const dms = workspaceNav(page).getByRole("region", { name: "Direct messages" });
  await dms.getByRole("button", { name: "Message someone privately" }).click();

  const dialog = page.getByRole("dialog", { name: "New direct message" });
  await dialog.getByRole("button", { name: "Add Grace" }).click();
  await dialog.getByRole("button", { name: "Open DM" }).click();

  await expect(page.getByRole("heading", { name: "Grace", exact: true })).toBeVisible();
  await page.getByLabel("Message Grace").fill("just between us");
  await page.getByLabel("Message Grace").press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "just between us" })).toBeVisible();
  await expect(dms.getByRole("button", { name: /^Grace/ })).toBeVisible();
  await expect(workspaceNav(page).getByRole("region", { name: "Channels" })).not.toContainText("Grace");
});

test("a channel someone else made with you in it shows up locked, ready to write in", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receivePrivateChannel(grace, "secret-plans");
  });
  await workspaceNav(page)
    .getByRole("button", { name: /^Private secret-plans/ })
    .click();
  await expect(page.getByLabel("Message #secret-plans")).toBeVisible();
  await page.getByRole("button", { name: "People in #secret-plans" }).click();
  const people = page.getByRole("dialog", { name: "People in #secret-plans" });
  await expect(people.getByRole("button", { name: "Add people" })).toBeHidden();
  await expect(people.getByRole("button", { name: "Leave channel" })).toBeVisible();
});
