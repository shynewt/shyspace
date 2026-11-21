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
  await expect(page.locator("header").getByLabel("Private channel", { exact: true })).toBeVisible();
  await expect(workspaceNav(page).getByRole("button", { name: /^launch-plans, private channel/ })).toBeVisible();
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
  await page.getByRole("dialog", { name: "Make Grace the owner?" }).getByRole("button", { name: "Make owner" }).click();
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
  await expect(dms.getByRole("button", { name: /^Grace, direct message/ })).toBeVisible();
  await expect(workspaceNav(page).getByRole("region", { name: "Channels" })).not.toContainText("Grace");
});

test("a channel someone else made with you in it shows up locked, ready to write in", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receivePrivateChannel(grace, "secret-plans");
  });
  await workspaceNav(page)
    .getByRole("button", { name: /^secret-plans, private channel/ })
    .click();
  await expect(page.getByLabel("Message #secret-plans")).toBeVisible();
  await page.getByRole("button", { name: "People in #secret-plans" }).click();
  const people = page.getByRole("dialog", { name: "People in #secret-plans" });
  await expect(people.getByRole("button", { name: "Add people" })).toBeHidden();
  await expect(people.getByRole("button", { name: "Leave channel" })).toBeVisible();
});

test("being removed while reading hides both composers and every action", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receivePrivateChannel(grace, "ops");
    window.fake.receiveMessage(grace, "ops", "Deploy at noon?");
  });
  await workspaceNav(page)
    .getByRole("button", { name: /^ops, private channel/ })
    .click();
  const message = page.getByRole("article").filter({ hasText: "Deploy at noon?" });
  await message.hover();
  await message.getByRole("button", { name: "Reply in thread" }).click();
  await expect(page.getByRole("textbox", { name: "Reply in thread" })).toBeVisible();

  await page.evaluate(() => window.fake.removeMeFromPrivateChannel("ops"));

  await expect(page.getByLabel("Message #ops")).toBeHidden();
  await expect(page.getByRole("textbox", { name: "Reply in thread" })).toBeHidden();
  await expect(page.getByText("You're no longer in this channel.").first()).toBeVisible();
  await page.getByRole("button", { name: "People in #ops" }).click();
  const people = page.getByRole("dialog", { name: "People in #ops" });
  await expect(people.getByText("This is who was in it when you left.")).toBeVisible();
  await expect(people.getByRole("button", { name: "Leave channel" })).toBeHidden();
});

test("someone who makes you the owner lets you add people", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.addMember("Alan");
    window.fake.receivePrivateChannel(grace, "ops");
  });
  await workspaceNav(page)
    .getByRole("button", { name: /^ops, private channel/ })
    .click();
  await page.getByRole("button", { name: "People in #ops" }).click();
  const people = page.getByRole("dialog", { name: "People in #ops" });
  await expect(people.getByRole("button", { name: "Add people" })).toBeHidden();

  await page.evaluate(() => window.fake.makeMeOwnerOf("ops"));
  await people.getByRole("button", { name: "Add people" }).click();
  await people.getByRole("button", { name: "Add Alan" }).click();
  await expect(people.getByRole("button", { name: "Remove Alan" })).toHaveAttribute("aria-pressed", "true");
});

test("opening a DM with the same person again finds the same one", async ({ page }) => {
  await page.evaluate(() => window.fake.addMember("Grace"));
  const dms = workspaceNav(page).getByRole("region", { name: "Direct messages" });
  for (let round = 0; round < 2; round += 1) {
    await dms.hover();
    await dms.getByRole("button", { name: "New direct message" }).click();
    const dialog = page.getByRole("dialog", { name: "New direct message" });
    await dialog.getByRole("button", { name: "Add Grace" }).click();
    await dialog.getByRole("button", { name: "Open DM" }).click();
  }
  await expect(dms.getByRole("button", { name: /^Grace, direct message/ })).toHaveCount(1);

  await page.keyboard.press("Control+k");
  await page.keyboard.type("Grace");
  await expect(page.getByRole("option", { name: /Grace/ }).first()).toBeVisible();
});

test("a DM can have at most eight other people", async ({ page }) => {
  await page.evaluate(() => {
    for (const name of ["A1", "A2", "A3", "A4", "A5", "A6", "A7", "A8", "A9"]) window.fake.addMember(name);
  });
  await workspaceNav(page)
    .getByRole("region", { name: "Direct messages" })
    .getByRole("button", { name: "Message someone privately" })
    .click();
  const dialog = page.getByRole("dialog", { name: "New direct message" });
  for (const name of ["A1", "A2", "A3", "A4", "A5", "A6", "A7", "A8"]) {
    await dialog.getByRole("button", { name: `Add ${name}`, exact: true }).click();
  }
  await expect(dialog.getByRole("button", { name: "Open DM" })).toBeEnabled();
  await dialog.getByRole("button", { name: "Add A9", exact: true }).click();
  await expect(dialog.getByText("A DM can have up to 8 other people.")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Open DM" })).toBeDisabled();
});

test("a DM with someone who left the workspace becomes read-only", async ({ page }) => {
  const grace = await page.evaluate(() => window.fake.addMember("Grace"));
  await workspaceNav(page)
    .getByRole("region", { name: "Direct messages" })
    .getByRole("button", { name: "Message someone privately" })
    .click();
  const dialog = page.getByRole("dialog", { name: "New direct message" });
  await dialog.getByRole("button", { name: "Add Grace" }).click();
  await dialog.getByRole("button", { name: "Open DM" }).click();
  await expect(page.getByLabel("Message Grace")).toBeVisible();

  await page.evaluate((member) => window.fake.removeFromWorkspace(member), grace);
  await expect(page.getByText("Grace left the workspace, so this conversation is read-only.")).toBeVisible();
  await expect(page.getByLabel("Message Grace")).toBeHidden();
});

test("someone who hasn't opened a channel yet shows as invited", async ({ page }) => {
  const alan = await page.evaluate(() => {
    const id = window.fake.addMember("Alan");
    window.fake.receivePrivateChannel(window.fake.addMember("Grace"), "ops", [id]);
    return id;
  });
  await workspaceNav(page)
    .getByRole("button", { name: /^ops, private channel/ })
    .click();
  await page.getByRole("button", { name: "People in #ops" }).click();
  const people = page.getByRole("dialog", { name: "People in #ops" });
  await expect(people.getByText("Invited", { exact: true })).toHaveCount(1);
  await expect(people.getByRole("listitem").filter({ hasText: "Alan" })).toContainText("Invited");
  await expect(people.getByText("Invited people haven't read anything here yet")).toBeVisible();

  await page.evaluate((id) => window.fake.privateMemberJoined("ops", id), alan);
  await expect(people.getByText("Invited", { exact: true })).toHaveCount(0);
  await expect(people.getByText("Invited people haven't read anything here yet")).toBeHidden();
});

test("missing a new key says so above the composer", async ({ page }) => {
  await page.evaluate(() => window.fake.receivePrivateChannel(window.fake.addMember("Grace"), "ops"));
  await workspaceNav(page)
    .getByRole("button", { name: /^ops, private channel/ })
    .click();
  await expect(page.getByText("your device didn't get it")).toBeHidden();

  await page.evaluate(() => window.fake.missNewKeyIn("ops"));
  await expect(page.getByText("You can't read new messages here")).toHaveCount(1);
  await expect(page.getByLabel("Message #ops")).toBeVisible();
});
