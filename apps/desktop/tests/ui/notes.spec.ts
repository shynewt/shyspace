import { expect, test } from "@playwright/test";

test("your notes never show up as a channel in the sidebar's sections", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  const sections = page.getByRole("navigation", { name: "Workspace" }).getByRole("region", { name: "Channels" });
  const buttons = sections.getByRole("button");
  const createChannel = async (name: string) => {
    await page.evaluate((channelName) => window.fake.handle("create_channel", { name: channelName }), name);
    await expect(sections.getByRole("button", { name: new RegExp(`^${name}`) })).toBeVisible();
  };
  const atStart = await buttons.count();
  await createChannel("first");
  const perChannel = (await buttons.count()) - atStart;

  await page.evaluate(() => window.fake.handle("open_notes", undefined));
  await createChannel("second");

  await expect(buttons).toHaveCount(atStart + 2 * perChannel);
});

test("notes are pinned first in Direct messages and open a place only you can see", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => window.fake.addMember("Grace"));
  const dms = page.getByRole("navigation", { name: "Workspace" }).getByRole("region", { name: "Direct messages" });

  await dms.getByRole("button", { name: /^Notes, only you can see them/ }).click();

  await expect(page.getByRole("heading", { name: "Notes", exact: true })).toBeVisible();
  await expect(page.getByText("Only you can see them")).toBeVisible();
  await expect(page.getByText("Jot down links, drafts and to-dos.")).toBeVisible();
  await expect(page.getByRole("button", { name: /^People in/ })).toBeHidden();
  const composer = page.getByLabel("Write a note");
  await composer.fill("call the bank");
  await composer.press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "call the bank" })).toBeVisible();
  await expect(dms.getByRole("button", { name: /^Notes/ })).toHaveAttribute("aria-current", "page");
});

test("opening notes again finds the same ones", async ({ page }) => {
  await page.goto("");
  const dms = page.getByRole("navigation", { name: "Workspace" }).getByRole("region", { name: "Direct messages" });
  await dms.getByRole("button", { name: /^Notes/ }).click();
  await page.getByLabel("Write a note").fill("first note");
  await page.getByLabel("Write a note").press("Enter");
  await page
    .getByRole("navigation", { name: "Workspace" })
    .getByRole("button", { name: /^general/ })
    .click();

  await dms.getByRole("button", { name: /^Notes/ }).click();

  await expect(page.getByRole("article").filter({ hasText: "first note" })).toBeVisible();
  await expect(dms.getByRole("button", { name: /^Notes/ })).toHaveCount(1);
});

test("notes have no alert level to set", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() => window.fake.handle("open_notes", undefined));
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Notifications" }).click();

  await expect(settings.getByRole("radiogroup", { name: "Alerts for #general" })).toBeVisible();
  await expect(settings.getByRole("radiogroup", { name: "Alerts for your notes" })).toHaveCount(0);
});
