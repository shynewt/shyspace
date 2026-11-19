import { expect, type Page, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #random")).toBeVisible();
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() => window.fake.addMember("Grace"));
});

async function graceSays(page: Page, channel: string, text: string) {
  await page.evaluate(
    ([channel, text]) => {
      const me = window.fake.session?.me ?? "";
      window.fake.receiveMessage(window.fake.memberNamed("Grace"), channel, text.replace("{me}", me));
    },
    [channel, text],
  );
}

const alertsShown = (page: Page) => page.evaluate(() => window.fake.alertsShown);

async function setLevel(page: Page, channel: string, choice: string) {
  await page.getByRole("button", { name: channel, exact: true }).hover();
  await page.getByRole("button", { name: `Actions for #${channel}` }).click();
  await page.getByRole("menuitem", { name: choice }).click();
}

test("a mention elsewhere shows a desktop notification", async ({ page }) => {
  await graceSays(page, "random", "[@Ada](shy://member/{me}) can you review?");

  await expect.poll(() => alertsShown(page)).toEqual([{ title: "Grace in #random", body: "@Ada can you review?" }]);
});

test("a muted channel stays dim and silent", async ({ page }) => {
  await setLevel(page, "random", "Mute channel");
  await graceSays(page, "random", "[@Ada](shy://member/{me}) muted mention");
  await graceSays(page, "random", "just chatting");

  await expect(page.getByRole("button", { name: "random, 2 unread, 1 mentions you" })).toBeVisible();
  expect(await alertsShown(page)).toEqual([]);
});

test("every message alerts when you choose so", async ({ page }) => {
  await setLevel(page, "random", "Notify for every message");
  await graceSays(page, "random", "lunch?");

  await expect.poll(() => alertsShown(page)).toEqual([{ title: "Grace in #random", body: "lunch?" }]);
});
