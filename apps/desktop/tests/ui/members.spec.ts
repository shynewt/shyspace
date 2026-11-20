import { expect, type Page, test } from "@playwright/test";

function settingsDialog(page: Page) {
  return page.getByRole("dialog", { name: "Settings" });
}

/** The tabs inside Settings › Members, not the section list beside them. */
function membersTabs(page: Page) {
  return settingsDialog(page).getByRole("tablist", { name: "Members" });
}

async function openWorkspaceMenu(page: Page) {
  await page.getByRole("button", { name: /^Acme/ }).click();
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a waiting join request shows a badge until it is approved", async ({ page }) => {
  const workspaceMenu = page.getByRole("button", { name: /^Acme/ });
  await expect(workspaceMenu.getByLabel(/waiting/)).toHaveCount(0);

  await page.evaluate(() => window.fake.receiveJoinRequest("Claude", { kind: "bot" }));
  await expect(workspaceMenu.getByLabel("1 join request waiting")).toBeVisible();

  await workspaceMenu.click();
  await page.getByRole("menuitem", { name: /Join requests/ }).click();
  const settings = settingsDialog(page);
  await expect(settings.getByRole("tab", { name: "Members 1 join request waiting" })).toBeVisible();
  await expect(membersTabs(page).getByRole("tab", { name: /Requests/ })).toHaveAttribute("aria-selected", "true");
  await expect(settings.getByText("1234 5678 9012 3456 7890")).toBeVisible();
  await expect(settings.getByText(/end-to-end encryption ends there/)).toBeVisible();
  await settings.getByRole("button", { name: "Codes match, let in" }).click();

  await expect(settings.getByText("Claude wants to join")).toHaveCount(0);
  await expect(settings.getByText("No one is waiting to join.")).toBeVisible();
  await membersTabs(page).getByRole("tab", { name: "Members" }).click();
  await expect(settings.getByRole("listitem").filter({ hasText: "Claude" })).toBeVisible();
  await expect(settings.getByLabel(/waiting/)).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(workspaceMenu.getByLabel(/waiting/)).toHaveCount(0);
});

test("invite people opens the invites, where a code is made to copy", async ({ page }) => {
  await openWorkspaceMenu(page);
  await page.getByRole("menuitem", { name: "Invite people" }).click();
  await expect(membersTabs(page).getByRole("tab", { name: "Invites" })).toHaveAttribute("aria-selected", "true");

  const settings = settingsDialog(page);
  await settings.getByLabel("Who is it for?").fill("Grace");
  await settings.getByRole("button", { name: "Create invite" }).click();

  await expect(settings.getByRole("button", { name: "Copy invite code" })).toBeVisible();
  await expect(settings.getByText(/^shy2_/)).toBeVisible();
  await expect(settings.getByRole("listitem").filter({ hasText: "Grace" })).toContainText("Expires");
});

test("an unused invite can be revoked", async ({ page }) => {
  await openWorkspaceMenu(page);
  await page.getByRole("menuitem", { name: "Invite people" }).click();
  const settings = settingsDialog(page);
  await settings.getByLabel("Who is it for?").fill("Grace");
  await settings.getByRole("button", { name: "Create invite" }).click();
  const invite = settings.getByRole("listitem").filter({ hasText: "Grace" });

  await invite.getByRole("button", { name: "Revoke the invite for Grace" }).click();
  await expect(invite).toHaveCount(0);
  expect(await page.evaluate(() => window.fake.handle("invites", undefined))).toContainEqual(
    expect.objectContaining({ label: "Grace", state: "revoked" }),
  );
});

test("the palette invites people too", async ({ page }) => {
  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("invite");
  await page.getByRole("option", { name: "Invite people" }).click();

  await expect(membersTabs(page).getByRole("tab", { name: "Invites" })).toHaveAttribute("aria-selected", "true");
});

test("escape closes settings right away", async ({ page }) => {
  await openWorkspaceMenu(page);
  await page.getByRole("menuitem", { name: "Workspace settings" }).click();
  await expect(membersTabs(page)).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(settingsDialog(page)).toBeHidden();
});

test("the member list says who's who and can be searched", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.addMember("Grace Hopper");
    window.fake.addMember("Linus");
    window.fake.addMember("Claude", { kind: "bot" });
  });
  await page.keyboard.press("Control+,");
  const settings = settingsDialog(page);
  await settings.getByRole("tab", { name: "Members" }).first().click();

  const rows = settings.getByRole("list", { name: "Members" }).getByRole("listitem");
  await expect(rows.filter({ hasText: "Ada" })).toContainText("Admin");
  await expect(rows.filter({ hasText: "Ada" })).toContainText("(you)");
  await expect(rows.filter({ hasText: "Claude" })).toContainText("Bot");

  await settings.getByLabel("Search members").fill("gr");
  await expect(rows).toHaveCount(1);
  await expect(rows).toContainText("Grace Hopper");
  await settings.getByLabel("Search members").fill("nobody");
  await expect(settings.getByText("No one matches “nobody”.")).toBeVisible();
});

test("the admin can remove a member after confirming", async ({ page }) => {
  await page.evaluate(() => window.fake.addMember("Grace"));
  await page.keyboard.press("Control+,");
  const settings = settingsDialog(page);
  await settings.getByRole("tab", { name: "Members" }).first().click();

  await settings.getByRole("button", { name: "Remove Grace" }).click();
  const confirm = page.getByRole("dialog", { name: "Remove Grace from Acme?" });
  await confirm.getByRole("button", { name: "Remove", exact: true }).click();

  await expect(confirm).toBeHidden();
  await expect(settings.getByRole("list", { name: "Former members" })).toContainText("Grace");
  await expect(settings.getByRole("button", { name: "Remove Grace" })).toHaveCount(0);
});

test("agents are listed with what they can read and how to add one", async ({ page }) => {
  await page.evaluate(() => window.fake.addMember("Robot", { kind: "bot" }));
  await page.keyboard.press("Control+,");
  const settings = settingsDialog(page);
  await settings.getByRole("tab", { name: "Agents" }).click();

  await expect(settings.getByText(/can read every channel and page/)).toBeVisible();
  await expect(settings.getByRole("list", { name: "Agents" })).toContainText("Robot");
  await settings.getByLabel("Agent name").fill("Claude");
  await settings.getByRole("button", { name: "Create agent invite" }).click();
  await expect(settings.getByText(/shy join --bot --name 'Claude' shy2_/)).toBeVisible();
});

test.describe("as a member", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("?start=member");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("members can see everyone but not invite or remove", async ({ page }) => {
    await page.getByRole("button", { name: /^Team/ }).click();
    await expect(page.getByRole("menuitem", { name: "Invite people" })).toHaveCount(0);
    await page.getByRole("menuitem", { name: "Workspace settings" }).click();

    const settings = settingsDialog(page);
    await expect(settings.getByRole("listitem").filter({ hasText: "Ada" })).toContainText("Admin");
    await expect(settings.getByRole("tablist", { name: "Members" })).toHaveCount(0);
    await expect(settings.getByRole("button", { name: /^Remove/ })).toHaveCount(0);
    await settings.getByRole("tab", { name: "Agents" }).click();
    await expect(settings.getByText("Ask Ada to add an agent.")).toBeVisible();
  });
});
