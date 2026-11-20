import { expect, type Page, test } from "@playwright/test";
import { chooseInSubmenu } from "./support";

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
  await chooseInSubmenu(page, "Notifications", choice);
}

test("a mention elsewhere shows a desktop notification", async ({ page }) => {
  await graceSays(page, "random", "[@Ada](shy://member/{me}) can you review?");

  await expect.poll(() => alertsShown(page)).toEqual([{ title: "Grace in #random", body: "@Ada can you review?" }]);
});

test("a muted channel stays dim and silent", async ({ page }) => {
  await setLevel(page, "random", "Nothing (muted)");
  await graceSays(page, "random", "[@Ada](shy://member/{me}) muted mention");
  await graceSays(page, "random", "just chatting");

  await expect(page.getByRole("button", { name: "random, muted, 2 unread, 1 mentions you" })).toBeVisible();
  expect(await alertsShown(page)).toEqual([]);
});

test("mentions only stays quiet for plain messages and shows an @", async ({ page }) => {
  await setLevel(page, "random", "Mentions only");
  await expect(page.getByRole("button", { name: "random, mentions only" })).toBeVisible();
  await graceSays(page, "random", "lunch?");
  await graceSays(page, "random", "[@Ada](shy://member/{me}) lunch with me?");

  await expect.poll(() => alertsShown(page)).toEqual([{ title: "Grace in #random", body: "@Ada lunch with me?" }]);
});

test("a right-click opens the channel's menu with the current level checked", async ({ page }) => {
  await page.getByRole("button", { name: "random", exact: true }).click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Rename" })).toBeVisible();
  await page.getByRole("menuitem", { name: "Notifications" }).hover();
  await expect(page.getByRole("menuitemradio", { name: "Every message" })).toHaveAttribute("aria-checked", "true");
});

test("every channel's alerts can be set and reset in Settings", async ({ page }) => {
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Notifications" }).click();
  await expect(settings.getByRole("tabpanel")).toContainText("Your status can make alerts quieter");
  const random = settings.getByRole("radiogroup", { name: "Alerts for #random" });
  const general = settings.getByRole("radiogroup", { name: "Alerts for #general" });

  await random.getByRole("radio", { name: "Muted" }).click();
  await general.getByRole("radio", { name: "Mentions only" }).click();
  await expect
    .poll(() => page.evaluate(() => window.fake.handle("channels", undefined)))
    .toContainEqual(expect.objectContaining({ name: "random", notify: "nothing" }));

  await settings.getByRole("button", { name: "Reset all to every message" }).click();
  await expect(random.getByRole("radio", { name: "Every message" })).toBeChecked();
  await expect(general.getByRole("radio", { name: "Every message" })).toBeChecked();
  await expect(settings.getByRole("button", { name: "Reset all to every message" })).toBeDisabled();

  await settings.getByRole("button", { name: "Undo" }).click();
  await expect(random.getByRole("radio", { name: "Muted" })).toBeChecked();
  await expect(general.getByRole("radio", { name: "Mentions only" })).toBeChecked();
});
