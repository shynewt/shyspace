import { expect, type Page, test } from "@playwright/test";

declare global {
  interface Window {
    /** The commands the app sent, in order, once a test starts recording. */
    commandsSent?: string[];
  }
}

function rail(page: Page) {
  return page.getByRole("navigation", { name: "Workspaces" });
}

/** The menu named after the shown workspace, at the top of the sidebar. */
function workspaceMenu(page: Page, name: string) {
  return page.getByRole("button", { name, exact: true }).and(page.locator("[aria-haspopup='menu']"));
}

async function openApp(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
}

/** A second workspace, "Team", on this device; returns its id. */
async function addTeam(page: Page): Promise<string> {
  const id = await page.evaluate(() => window.fake.workspaces.add("Team"));
  await expect(rail(page)).toBeVisible();
  return id;
}

test("with one workspace there's no rail", async ({ page }) => {
  await openApp(page);
  await expect(rail(page)).toHaveCount(0);
});

test("from two workspaces, the rail shows them, and a click or Ctrl+number switches", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await expect(rail(page).getByRole("button", { name: /^Acme/ })).toHaveAttribute("aria-current", "page");

  await rail(page).getByRole("button", { name: /^Team/ }).click();
  await expect(rail(page).getByRole("button", { name: /^Team/ })).toHaveAttribute("aria-current", "page");
  await expect(workspaceMenu(page, "Team")).toBeVisible();

  await page.keyboard.press("Control+1");
  await expect(rail(page).getByRole("button", { name: /^Acme/ })).toHaveAttribute("aria-current", "page");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("another workspace shows a dot for unread and a count for mentions, and nothing while muted", async ({ page }) => {
  await openApp(page);
  const team = await addTeam(page);
  const icon = rail(page).getByRole("button", { name: /^Team/ });

  await page.evaluate((id) => window.fake.workspaces.setBadge(id, true, 0), team);
  await expect(icon).toHaveAccessibleName("Team, unread");
  await page.evaluate((id) => window.fake.workspaces.setBadge(id, true, 3), team);
  await expect(icon).toHaveAccessibleName("Team, 3 mentions");
  await expect(icon).toContainText("3");

  await icon.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Mute notifications" }).click();
  await expect(icon).toHaveAccessibleName("Team, muted");
  await expect(icon).not.toContainText("3");

  await icon.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Mark all as read" }).click();
  await expect.poll(() => page.evaluate(() => window.fake.workspaces.markedRead)).toEqual([team]);
});

test("a notification from another workspace names it, and opens it", async ({ page }) => {
  await openApp(page);
  const team = await addTeam(page);
  await page.evaluate((id) => window.fake.alertFrom(id, "Grace", "design", "The mockups are up"), team);

  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown))
    .toEqual([{ title: "Team · Grace in #design", body: "The mockups are up" }]);
  await page.evaluate(() => window.fake.clickAlert(0));
  await expect(rail(page).getByRole("button", { name: /^Team/ })).toHaveAttribute("aria-current", "page");
});

test("a muted workspace, or the pause, holds its notifications back", async ({ page }) => {
  await openApp(page);
  const team = await addTeam(page);

  await page.getByRole("button", { name: /^Ada\b/ }).click();
  await page.getByRole("menuitem", { name: "Pause notifications everywhere" }).click();
  await page.getByRole("menuitemradio", { name: "For 1 hour" }).click();
  await expect(page.getByRole("img", { name: "Notifications paused" })).toBeVisible();
  await page.evaluate((id) => window.fake.alertFrom(id, "Grace", "design", "Paused"), team);
  await expect.poll(() => page.evaluate(() => window.fake.alertsHeld)).toEqual(["Paused"]);

  await page.getByRole("button", { name: /^Ada\b/ }).click();
  await page.getByRole("menuitem", { name: /^Resume notifications/ }).click();
  await page.evaluate((id) => window.fake.alertFrom(id, "Grace", "design", "Back on"), team);
  await expect.poll(() => page.evaluate(() => window.fake.alertsShown.map((alert) => alert.body))).toEqual(["Back on"]);
});

test("leaving asks first, and with one workspace left the rail goes", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await rail(page).getByRole("button", { name: /^Team/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Leave workspace…" }).click();
  const confirm = page.getByRole("dialog", { name: "Leave Team?" });
  await expect(confirm).toContainText("removed from this device");
  await confirm.getByRole("button", { name: "Leave workspace" }).click();
  await expect(rail(page)).toHaveCount(0);
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a removed member can keep reading, then leave and join again with a new invite", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => window.fake.removeMeFromWorkspace());
  const notice = page.getByRole("dialog", { name: "You were removed from this workspace" });
  await expect(notice.getByRole("button", { name: "Leave workspace" })).toBeVisible();

  await notice.getByRole("button", { name: "Keep reading" }).click();
  await expect(notice).toBeHidden();
  await expect(workspaceMenu(page, "Acme")).toBeVisible();

  await workspaceMenu(page, "Acme").click();
  await page.getByRole("menuitem", { name: "Leave workspace…" }).click();
  await page.getByRole("dialog", { name: "Leave Acme?" }).getByRole("button", { name: "Leave workspace" }).click();
  await page.getByRole("radio", { name: "Join" }).click();
  await page.getByLabel("Invite code").fill("shy3_new-invite");
  await page.getByLabel("Your name").fill("Ada");
  await page.getByRole("button", { name: /Ask to join/ }).click();

  await expect(workspaceMenu(page, "Team")).toBeVisible();
  await expect(page.getByRole("dialog", { name: "You were removed from this workspace" })).toHaveCount(0);
});

test("a removed member can leave straight from the notice", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await page.evaluate(() => window.fake.removeMeFromWorkspace());

  await page
    .getByRole("dialog", { name: "You were removed from this workspace" })
    .getByRole("button", { name: "Leave workspace" })
    .click();
  await page
    .getByRole("dialog", { name: /^Leave / })
    .getByRole("button", { name: "Leave workspace" })
    .click();

  await expect(rail(page)).toHaveCount(0);
});

test("leaving the shown workspace shows the next one", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await workspaceMenu(page, "Acme").click();
  await page.getByRole("menuitem", { name: "Leave workspace…" }).click();
  await page.getByRole("dialog", { name: "Leave Acme?" }).getByRole("button", { name: "Leave workspace" }).click();
  await expect(workspaceMenu(page, "Team")).toBeVisible();
  await expect(rail(page)).toHaveCount(0);
});

test("the + opens adding a workspace, with a way back", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await rail(page).getByRole("button", { name: "Add a workspace" }).click();
  await expect(page.getByRole("heading", { name: "Add a workspace" })).toBeVisible();
  await page.getByRole("button", { name: "Back to Acme" }).click();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect(rail(page)).toBeVisible();
});

test("a leave that fails says why and keeps the workspace", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await page.evaluate(() => {
    window.fake.workspaces.leaveFails = "the workspace's folder is still in use";
  });
  await rail(page).getByRole("button", { name: /^Team/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Leave workspace…" }).click();
  const confirm = page.getByRole("dialog", { name: "Leave Team?" });
  await confirm.getByRole("button", { name: "Leave workspace" }).click();
  await expect(page.getByText("the workspace's folder is still in use")).toBeVisible();
  await expect(confirm).toBeVisible();
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(rail(page).getByRole("button", { name: /^Team/ })).toBeVisible();
});

/** Records the commands the app sends, in order, from now on. */
async function recordCommands(page: Page) {
  await page.evaluate(() => {
    const answer = window.fake.handle.bind(window.fake);
    window.commandsSent = [];
    window.fake.handle = (command, args) => {
      window.commandsSent?.push(command);
      return answer(command, args);
    };
  });
}

/** Whether `first` was sent before `then`, both at least once. */
function sentBefore(page: Page, first: string, then: string) {
  return page.evaluate(
    ([first, then]) => {
      const sent = window.commandsSent ?? [];
      const firstAt = sent.indexOf(first);
      const thenAt = sent.indexOf(then);
      return firstAt !== -1 && thenAt !== -1 && firstAt < thenAt;
    },
    [first, then],
  );
}

test("a draft typed just before switching is saved in its own workspace first", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await recordCommands(page);
  await page.getByLabel("Message #general").fill("half a thought");
  await page.keyboard.press("Control+2");
  await expect(rail(page).getByRole("button", { name: /^Team/ })).toHaveAttribute("aria-current", "page");
  expect(await sentBefore(page, "set_draft", "select_workspace")).toBe(true);
});

test("page typing just before switching is saved in its own workspace first", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => window.fake.seedPage("Plans", "Start"));
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Plans", exact: true }).click();
  await addTeam(page);
  await recordCommands(page);
  await page.locator(".cm-content").click();
  await page.keyboard.type(" more");
  await page.keyboard.press("Control+2");
  await expect(rail(page).getByRole("button", { name: /^Team/ })).toHaveAttribute("aria-current", "page");
  expect(await sentBefore(page, "apply_doc_update", "select_workspace")).toBe(true);
});

test("what you read just before switching is saved in its own workspace first", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await recordCommands(page);
  await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "general", "Fresh news"));
  await expect(page.getByText("Fresh news")).toBeVisible();
  await page.keyboard.press("Control+2");
  await expect(rail(page).getByRole("button", { name: /^Team/ })).toHaveAttribute("aria-current", "page");
  expect(await sentBefore(page, "mark_read", "select_workspace")).toBe(true);
});

test("each workspace reopens where you were in it", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => window.fake.seedPage("Plans", "Start"));
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Plans", exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue("Plans");
  await addTeam(page);

  await page.keyboard.press("Control+2");
  await expect(workspaceMenu(page, "Team")).toBeVisible();
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.keyboard.press("Control+1");
  await expect(page.getByLabel("Page title")).toHaveValue("Plans");
});

test("notifications keep coming while adding a workspace, and one opens its workspace", async ({ page }) => {
  await openApp(page);
  const team = await addTeam(page);
  await rail(page).getByRole("button", { name: "Add a workspace" }).click();
  await expect(page.getByRole("heading", { name: "Add a workspace" })).toBeVisible();

  await page.evaluate((id) => window.fake.alertFrom(id, "Grace", "design", "While you were away"), team);
  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown.map((alert) => alert.title)))
    .toEqual(["Team · Grace in #design"]);
  await page.evaluate(() => window.fake.clickAlert(0));
  await expect(rail(page).getByRole("button", { name: /^Team/ })).toHaveAttribute("aria-current", "page");
});

test("a call stays reachable while another workspace is shown", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();
  await expect(page.getByRole("region", { name: "Your huddle" })).toContainText("general");

  await page.keyboard.press("Control+2");
  await expect(workspaceMenu(page, "Team")).toBeVisible();
  const bar = page.getByRole("region", { name: "Your huddle" });
  await expect(bar).toContainText("Acme · #general");
  await bar.getByRole("button", { name: "Mute", exact: true }).click();
  await expect(bar.getByRole("button", { name: "Mute", exact: true })).toHaveAttribute("aria-pressed", "true");

  await bar.getByRole("button", { name: "Acme · #general" }).click();
  await expect(workspaceMenu(page, "Acme")).toBeVisible();
  await page.getByRole("region", { name: "Your huddle" }).getByRole("button", { name: "Leave" }).click();
  await expect(page.getByRole("region", { name: "Your huddle" })).toHaveCount(0);
});

test("a ring from another workspace shows, and Join goes there to answer", async ({ page }) => {
  await openApp(page);
  const team = await addTeam(page);
  await page.evaluate((id) => window.fake.huddles.ringFrom(id, `${id}-grace`, "Grace"), team);
  const card = page.getByRole("region", { name: "Incoming huddle" });
  await expect(card).toContainText("Grace started a huddle");
  await expect(card).toContainText("in Team");

  await card.getByRole("button", { name: "Join" }).click();
  await expect(workspaceMenu(page, "Team")).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => window.fake.handle("my_huddle", undefined)))
    .toMatchObject({ workspace_id: team });
});

test("declining a ring from another workspace declines it there", async ({ page }) => {
  await openApp(page);
  const team = await addTeam(page);
  await page.evaluate((id) => window.fake.huddles.ringFrom(id, `${id}-grace`, "Grace"), team);
  await page.getByRole("region", { name: "Incoming huddle" }).getByRole("button", { name: "Decline" }).click();
  await expect(page.getByRole("region", { name: "Incoming huddle" })).toHaveCount(0);
  await expect(workspaceMenu(page, "Acme")).toBeVisible();
});

test("a channel in the shown workspace isn't taken for the call's, even with the same id", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();
  await page.keyboard.press("Control+2");
  await expect(workspaceMenu(page, "Team")).toBeVisible();
  // The fake shares channel ids across workspaces, as two workspaces' ids could collide.
  await expect(page.getByRole("main").getByRole("button", { name: "Leave the huddle" })).toHaveCount(0);
});

test("switching workspace drops an Undo that would act on the other one", async ({ page }) => {
  await openApp(page);
  await addTeam(page);
  await page.getByLabel("Message #general").fill("maybe not");
  await page
    .getByRole("navigation", { name: "Workspace" })
    .getByRole("button", { name: /^Drafts & sent/ })
    .click();
  const row = page.getByRole("list", { name: "Drafts" }).getByRole("listitem").filter({ hasText: "maybe not" });
  await row.hover();
  await row.getByRole("button", { name: "Discard draft" }).click();
  await expect(page.getByRole("button", { name: "Undo" })).toBeVisible();

  await page.keyboard.press("Control+2");
  await expect(workspaceMenu(page, "Team")).toBeVisible();
  await expect(page.getByRole("button", { name: "Undo" })).toHaveCount(0);
});
