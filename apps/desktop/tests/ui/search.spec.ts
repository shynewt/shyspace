import { expect, test } from "@playwright/test";

test("search finds a message and jumps to its channel", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", "The quarterly budget is ready");
  });

  await page.getByRole("button", { name: /Search/ }).click();
  await page.getByLabel("Search messages and pages").fill("budg");
  const hit = page.getByRole("option", { name: /quarterly budget/ });
  await expect(hit).toBeVisible();
  await page.keyboard.press("Enter");

  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect(page.getByRole("article").getByText("The quarterly budget is ready")).toBeVisible();
});
