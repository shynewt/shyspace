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
