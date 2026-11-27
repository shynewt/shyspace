import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("notification settings show a private channel with its lock", async ({ page }) => {
  await page.evaluate(() => window.fake.receivePrivateChannel(window.fake.addMember("Grace"), "ops"));
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Notifications" }).click();

  const row = settings.getByRole("list", { name: "Channel alerts" }).getByRole("listitem").filter({ hasText: "ops" });
  await expect(row.getByLabel("Private", { exact: true })).toBeVisible();
  await expect(row).not.toContainText("#ops");
});

test("the inbox shows where a mention was, lock and all", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receivePrivateChannel(grace, "ops");
    window.fake.receiveMessage(grace, "ops", `Look [@Ada](shy://member/${window.fake.session?.me ?? ""})`);
  });
  await page.keyboard.press("Control+Shift+A");

  const row = page.getByRole("button", { name: /mentioned you in/ });
  await expect(row.getByLabel("Private", { exact: true })).toBeVisible();
  await expect(row).toContainText("ops");
  await expect(row).not.toContainText("#ops");
});

test("a draft in a DM is named after the person", async ({ page }) => {
  await page.evaluate(() => {
    const dm = window.fake.handle("open_dm", { members: [window.fake.addMember("Grace")] });
    if (typeof dm === "string") window.fake.handle("set_draft", { target: dm, text: "half written" });
  });
  await page.getByRole("button", { name: "Drafts & sent, 1 draft" }).click();

  const row = page.getByRole("list", { name: "Drafts" }).getByRole("button", { name: /half written/ });
  await expect(row).not.toContainText("Thread in");
  await expect(row.getByText("Grace", { exact: true })).toBeVisible();
  await expect(row).not.toContainText("#");
});

test("a link to a DM message names the person, not a raw channel name", async ({ page }) => {
  const id = await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const dm = window.fake.handle("open_dm", { members: [grace] });
    return typeof dm === "string" ? window.fake.receiveMessageIn(grace, dm, "Between us") : "";
  });
  await page.getByLabel("Message #general").fill(`see shy://message/${id}`);
  await page.getByLabel("Message #general").press("Enter");

  const card = page.getByRole("button", { name: "Message from Grace in Grace" });
  await expect(card).toContainText("Grace in");
  await expect(card.getByText("Grace", { exact: true })).toHaveCount(2);
  await expect(card).not.toContainText("#");
});

test("a direct message whose others all left still has a name", async ({ page }) => {
  await page.evaluate(() => window.fake.dmTheyLeft(window.fake.addMember("Grace")));

  await expect(page.getByRole("button", { name: "Direct message, direct message", exact: true })).toBeVisible();
});
