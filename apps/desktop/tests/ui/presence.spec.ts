import { expect, type Page, test } from "@playwright/test";

async function openMembers(page: Page) {
  await page.getByRole("button", { name: /Members & invites/ }).click();
  return page.getByRole("dialog");
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("your status shows to everyone and can be cleared", async ({ page }) => {
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByLabel("Emoji").fill("🎧");
  await settings.getByLabel("Status", { exact: true }).fill("Deep work");
  await settings.getByRole("tab", { name: "Busy: none" }).click();
  await settings.getByRole("button", { name: "Set status" }).click();
  await expect(page.getByText("Status set.")).toBeVisible();
  await page.keyboard.press("Escape");

  const members = await openMembers(page);
  await expect(members.getByText("🎧 Deep work")).toBeVisible();
  await expect(members.getByText("· Busy")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(members).toBeHidden();

  await page.keyboard.press("Control+,");
  await page.getByRole("dialog", { name: "Settings" }).getByRole("button", { name: "Clear status" }).click();
  await expect(page.getByText("Status cleared.")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeHidden();
  await expect((await openMembers(page)).getByText("Deep work")).toHaveCount(0);
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
  await page.keyboard.press("Control+,");
  const visibility = page.getByRole("dialog", { name: "Settings" }).getByRole("tab", { name: "Appear offline" });
  await visibility.click();
  await expect(visibility).toHaveAttribute("aria-selected", "true");
  expect(await page.evaluate(() => window.fake.ownPresenceLog)).toEqual(["offline"]);
  await page.keyboard.press("Escape");

  const me = (await openMembers(page)).getByRole("listitem").filter({ hasText: "(you)" });
  await expect(me).toContainText("Not reachable right now");
});
