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

test("resetting alerts keeps what worked undoable when one channel fails", async ({ page }) => {
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Notifications" }).click();
  const random = settings.getByRole("radiogroup", { name: "Alerts for #random" });
  const general = settings.getByRole("radiogroup", { name: "Alerts for #general" });
  await random.getByRole("radio", { name: "Muted" }).click();
  await general.getByRole("radio", { name: "Mentions only" }).click();
  await expect(general.getByRole("radio", { name: "Mentions only" })).toBeChecked();
  await page.evaluate(() => {
    const answer = window.fake.handle.bind(window.fake);
    const channels = answer("channels", undefined) as { id: string; name: string }[];
    const randomId = channels.find((channel) => channel.name === "random")?.id;
    window.fake.handle = (command, args) => {
      if (command === "set_notify_level" && args && "channel" in args && args.channel === randomId) {
        throw "the workspace is offline";
      }
      return answer(command, args);
    };
  });

  await settings.getByRole("button", { name: "Reset all to every message" }).click();
  await expect(settings.getByRole("alert")).toContainText("the workspace is offline");
  await expect(general.getByRole("radio", { name: "Every message" })).toBeChecked();
  await expect(random.getByRole("radio", { name: "Muted" })).toBeChecked();

  await settings.getByRole("button", { name: "Undo" }).click();
  await expect(general.getByRole("radio", { name: "Mentions only" })).toBeChecked();
});

const alertPictures = (page: Page) => page.evaluate(() => window.fake.alertPictures);

test("a notification names the place: the person for a DM, the group, or the channel", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.memberNamed("Grace");
    const alan = window.fake.addMember("Alan");
    const dm = window.fake.handle("open_dm", { members: [grace] });
    const group = window.fake.handle("open_dm", { members: [grace, alan] });
    if (typeof dm !== "string" || typeof group !== "string") throw new Error("no DM");
    window.fake.receiveMessageIn(grace, dm, "just you");
    window.fake.receiveMessageIn(grace, group, "all of us");
    window.fake.receivePrivateChannel(grace, "ops");
    window.fake.receiveMessage(grace, "ops", "in private");
  });

  await expect
    .poll(() => alertsShown(page))
    .toEqual([
      { title: "Grace", body: "just you" },
      { title: "Grace in your group with Alan", body: "all of us" },
      { title: "Grace in #ops", body: "in private" },
    ]);
});

test("notifications show in the order their messages came, even when one is slow to prepare", async ({ page }) => {
  await page.evaluate(() => {
    // The first notification's lookup of the members answers late, as a busy core might.
    const answer = window.fake.handle.bind(window.fake);
    let isFirst = true;
    window.fake.handle = (command, args) => {
      if (command !== "members" || !isFirst) return answer(command, args);
      isFirst = false;
      const late = answer(command, args);
      return new Promise((resolve) => setTimeout(() => resolve(late), 300));
    };
    const grace = window.fake.memberNamed("Grace");
    window.fake.receiveMessage(grace, "random", "first");
    window.fake.receiveMessage(grace, "random", "second");
  });

  await expect.poll(() => alertsShown(page)).toHaveLength(2);
  expect((await alertsShown(page)).map((alert) => alert.body)).toEqual(["first", "second"]);
});

test("a notification shows the sender's picture", async ({ page }) => {
  await graceSays(page, "random", "hello");
  const grace = await page.evaluate(() => window.fake.memberNamed("Grace"));

  await expect.poll(() => alertPictures(page)).toEqual([grace]);
});
