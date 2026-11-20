import { expect, type Page, test } from "@playwright/test";
import { failWith } from "./support";

async function openMembers(page: Page) {
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Members" }).first().click();
  return settings.getByRole("list", { name: "Members" });
}

/** The button at the foot of the sidebar with your name and status. */
function meButton(page: Page) {
  return page.getByRole("button", { name: /^Ada\b/ });
}

async function openStatusDialog(page: Page) {
  await meButton(page).click();
  await page.getByRole("menuitem", { name: /^(Set|Edit) status$/ }).click();
  return page.getByRole("dialog", { name: "Set a status" });
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("your status shows to everyone and can be cleared", async ({ page }) => {
  const dialog = await openStatusDialog(page);
  await dialog.getByRole("button", { name: "Choose an emoji" }).click();
  await page.getByLabel("Search emoji").fill("headphone");
  await page.getByRole("gridcell", { name: "headphone" }).click();
  await dialog.getByLabel("Status", { exact: true }).fill("Deep work");
  await dialog.getByRole("radio", { name: "None", exact: true }).click();
  await dialog.getByRole("button", { name: "Set status" }).click();
  await expect(page.getByText("Status set.")).toBeVisible();
  await expect(dialog).toBeHidden();
  await expect(meButton(page)).toContainText("🎧 Deep work");

  const members = await openMembers(page);
  await expect(members.getByLabel("Status: 🎧 Deep work", { exact: true })).toBeVisible();
  await expect(members.getByText("Deep work")).toBeVisible();
  await expect(members.getByText("· Busy")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(members).toBeHidden();

  await meButton(page).click();
  await page.getByRole("menuitem", { name: "Clear status" }).click();
  await expect(page.getByText("Status cleared.")).toBeVisible();
  await expect(meButton(page)).not.toContainText("Deep work");
  await expect((await openMembers(page)).getByText("Deep work")).toHaveCount(0);
});

test("a status can let only mentions through", async ({ page }) => {
  const dialog = await openStatusDialog(page);
  await dialog.getByLabel("Status", { exact: true }).fill("Heads down");
  await dialog.getByRole("radio", { name: "Mentions only" }).click();
  await dialog.getByRole("button", { name: "Set status" }).click();
  await expect(page.getByText("Status set.")).toBeVisible();

  await expect((await openMembers(page)).getByText("· Mentions only")).toBeVisible();
});

test("a preset fills in the status to set", async ({ page }) => {
  const dialog = await openStatusDialog(page);
  await dialog.getByRole("button", { name: /Focusing/ }).click();
  await expect(dialog.getByLabel("Status", { exact: true })).toHaveValue("Focusing");
  await expect(dialog.getByRole("radio", { name: "4 hours" })).toBeChecked();
  await expect(dialog.getByRole("radio", { name: "Mentions only" })).toBeChecked();
  await dialog.getByRole("button", { name: "Set status" }).click();

  await expect(meButton(page)).toContainText("Focusing");
});

test("editing a status keeps when it clears", async ({ page }) => {
  let dialog = await openStatusDialog(page);
  await dialog.getByLabel("Status", { exact: true }).fill("Lunch");
  await dialog.getByRole("radio", { name: "1 hour" }).click();
  await dialog.getByRole("button", { name: "Set status" }).click();
  await expect(dialog).toBeHidden();
  const untilMs = () =>
    page.evaluate(() => {
      const me = window.fake.session?.me;
      const members = window.fake.handle("members", undefined) as { id: string; status: { until_ms: number } }[];
      return members.find((member) => member.id === me)?.status.until_ms;
    });
  const firstUntil = await untilMs();

  dialog = await openStatusDialog(page);
  await expect(dialog.getByRole("radio", { name: /^Until / })).toBeChecked();
  await dialog.getByLabel("Status", { exact: true }).fill("Long lunch");
  await dialog.getByRole("button", { name: "Set status" }).click();
  await expect(meButton(page)).toContainText("Long lunch");
  expect(await untilMs()).toBe(firstUntil);
});

test("a failed status save keeps what you typed and says why", async ({ page }) => {
  const dialog = await openStatusDialog(page);
  await failWith(page, "set_status", "the workspace is offline");
  await dialog.getByLabel("Status", { exact: true }).fill("Deep work");
  await dialog.getByRole("button", { name: "Set status" }).click();

  await expect(dialog.getByRole("alert")).toHaveText("the workspace is offline");
  await expect(dialog.getByLabel("Status", { exact: true })).toHaveValue("Deep work");
});

test("others show as online, away or not reachable", async ({ page }) => {
  const grace = await page.evaluate(() => window.fake.addMember("Grace"));
  const members = await openMembers(page);
  const row = members.getByRole("listitem").filter({ hasText: "Grace" });
  await expect(row).toContainText("Not reachable right now");

  await page.evaluate((id) => window.fake.setPresence(id, "online"), grace);
  await expect(row).toContainText("Online");
  await page.evaluate((id) => window.fake.setPresence(id, "away"), grace);
  await expect(row).toContainText("Away");
});

test("appearing offline stops telling others you're here", async ({ page }) => {
  await meButton(page).click();
  await page.getByRole("menuitem", { name: "Appear offline" }).click();
  await expect.poll(() => page.evaluate(() => window.fake.ownPresenceLog)).toEqual(["offline"]);
  await expect(meButton(page)).toContainText("Appearing offline");

  const me = (await openMembers(page)).getByRole("listitem").filter({ hasText: "(you)" });
  await expect(me).toContainText("Not reachable right now");
  await page.keyboard.press("Escape");

  await meButton(page).click();
  await page.getByRole("menuitem", { name: "Stop appearing offline" }).click();
  await expect.poll(() => page.evaluate(() => window.fake.ownPresenceLog)).toEqual(["offline", "visible"]);
  await expect(meButton(page)).toContainText("Online");
});

test("the me menu opens settings", async ({ page }) => {
  await meButton(page).click();
  await page.getByRole("menuitem", { name: "Settings" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" }).getByRole("tab", { name: "Profile" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("editing a status whose time has passed doesn't keep the old end", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.handle("set_status", { emoji: "", text: "Lunch", untilMs: Date.now() - 60_000, alerts: "all" }),
  );
  await expect(meButton(page)).toContainText("Lunch");

  const dialog = await openStatusDialog(page);
  await expect(dialog.getByRole("radio", { name: /^Until / })).toHaveCount(0);
  await expect(dialog.getByRole("radio", { name: "Never" })).toBeChecked();
});
