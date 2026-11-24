import { expect, type Page, test } from "@playwright/test";

/** In a DM every message is for you, so its count is plain; channels mark mentions with @. */

const sidebar = (page: Page) => page.getByRole("navigation", { name: "Workspace" });

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a DM's unread messages show as a plain count, not a mention", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const dm = window.fake.handle("open_dm", { members: [grace] });
    if (typeof dm !== "string") throw new Error("no DM");
    window.fake.receiveMessageIn(grace, dm, "Got a minute?");
  });
  const row = sidebar(page).getByRole("button", { name: "Grace, direct message, 1 unread" });
  await expect(row).toBeVisible();
  await expect(row.locator("..")).toContainText("1");
  await expect(row.locator("..")).not.toContainText("@");
});

test("a group DM counts like a DM", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const alan = window.fake.addMember("Alan");
    const group = window.fake.handle("open_dm", { members: [grace, alan] });
    if (typeof group !== "string") throw new Error("no group");
    window.fake.receiveMessageIn(grace, group, "one");
    window.fake.receiveMessageIn(alan, group, "two");
  });
  const row = sidebar(page).getByRole("button", { name: /direct message, 2 unread$/ });
  await expect(row).toBeVisible();
  await expect(row.locator("..")).not.toContainText("@");
});

test("a private channel marks a mention with @, like a channel", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receivePrivateChannel(grace, "ops");
    window.fake.receiveMessage(grace, "ops", `[@Ada](shy://member/${window.fake.session?.me}) look`);
  });
  const row = sidebar(page).getByRole("button", { name: "ops, private channel, 1 unread, 1 mentions you" });
  await expect(row).toBeVisible();
  await expect(row.locator("..")).toContainText("@1");
});

test("a channel marks a mention with @", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.handle("create_channel", { name: "random" });
    window.fake.receiveMessage(grace, "random", `[@Ada](shy://member/${window.fake.session?.me}) look`);
  });
  await expect(
    sidebar(page).getByRole("button", { name: "random, 1 unread, 1 mentions you" }).locator(".."),
  ).toContainText("@1");
});
