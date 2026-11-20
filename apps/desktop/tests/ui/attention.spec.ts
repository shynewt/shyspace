import { expect, test } from "@playwright/test";

test("what needs you shows outside the window, and clears once seen", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.fake.attentionCount)).toBe(0);

  await page.evaluate(() => {
    window.fake.handle("create_channel", { name: "random" });
    const grace = window.fake.addMember("Grace");
    const mention = window.fake.receiveMessage(grace, "random", `[@Ada](shy://member/${window.fake.session?.me}) look`);
    window.fake.handle("save_for_later", {
      item: mention,
      remindMs: Date.now() - 1000,
    });
  });

  await expect.poll(() => page.evaluate(() => window.fake.attentionCount)).toBe(2);

  await page.getByRole("button", { name: /^random/ }).click();
  await expect.poll(() => page.evaluate(() => window.fake.attentionCount)).toBe(1);
});
