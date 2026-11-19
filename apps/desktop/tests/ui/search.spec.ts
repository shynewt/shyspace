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

test("the palette runs actions", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("inbox");
  await page.getByRole("option", { name: /Open inbox/ }).click();

  await expect(page.getByRole("heading", { name: "Inbox" })).toBeVisible();
});

test("a reply found by search opens in its thread", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const question = window.fake.receiveMessage(grace, "general", "Which version ships?");
    window.fake.receiveReply(grace, question, "Version two ships Friday");
  });
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("Friday");
  await page.getByRole("option", { name: /Version two/ }).click();

  const thread = page.getByRole("complementary", { name: "Thread" });
  await expect(thread.getByRole("article").filter({ hasText: "Version two ships Friday" })).toBeVisible();
});
