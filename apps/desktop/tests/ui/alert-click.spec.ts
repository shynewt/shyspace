import { expect, test } from "@playwright/test";

test("clicking an alert opens the message it's about", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() => {
    window.fake.handle("create_channel", { name: "random" });
    window.fake.receiveMessage(window.fake.addMember("Grace"), "random", "Release plan for Friday");
  });
  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown.map((alert) => alert.body)))
    .toContain("Release plan for Friday");

  await page.evaluate(() => window.fake.clickAlert(window.fake.alertsShown.length - 1));

  await expect(page.getByRole("heading", { name: "random", exact: true })).toBeVisible();
  await expect(page.getByRole("article").filter({ hasText: "Release plan for Friday" })).toBeVisible();
});
