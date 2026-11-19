import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #random")).toBeVisible();
  await page.getByRole("button", { name: "general", exact: true }).click();
  const composer = page.getByLabel("Message #general");
  await composer.fill("Who reviews?");
  await composer.press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "Who reviews?" })).toBeVisible();
  await page.evaluate(() => window.fake.addMember("Grace"));
});

test("the inbox gathers mentions and thread replies and opens them in place", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.memberNamed("Grace");
    window.fake.receiveReply(grace, window.fake.messageWithText("Who reviews?"), "I can");
    window.fake.receiveMessage(grace, "random", `[@Ada](shy://member/${window.fake.session?.me}) look at this`);
  });

  const inboxLink = page.getByRole("button", { name: "Inbox, 2 unread" });
  await expect(inboxLink).toBeVisible();
  await inboxLink.click();
  const attention = page.getByRole("region", { name: "Mentions and replies" });
  await expect(attention.getByRole("button")).toHaveCount(2);
  await expect(attention.getByRole("button").first()).toContainText("mentioned you in #random");

  await attention.getByRole("button", { name: /replied in a thread/ }).click();
  const thread = page.getByRole("complementary", { name: "Thread" });
  await expect(thread.getByRole("article").filter({ hasText: "I can" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Inbox, 1 unread" })).toBeVisible();
});

test("mark all as read leaves you caught up", async ({ page }) => {
  await page.evaluate(() => window.fake.receiveMessage(window.fake.memberNamed("Grace"), "random", "plain news"));
  await page.getByRole("button", { name: "Inbox", exact: true }).click();
  await expect(page.getByRole("region", { name: "Unread channels" })).toContainText("random");

  await page.getByRole("button", { name: "Mark all as read" }).click();
  await expect(page.getByText("You're all caught up")).toBeVisible();
  await expect(page.getByRole("button", { name: "random", exact: true })).toBeVisible();
});
