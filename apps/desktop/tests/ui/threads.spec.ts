import { expect, type Page, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
  await page.evaluate(() => window.fake.addMember("Grace"));
});

async function openThreadOf(page: Page, text: string) {
  await page.getByRole("log", { name: "Messages in #general" }).getByRole("article").filter({ hasText: text }).hover();
  await page.getByRole("button", { name: "Reply in thread" }).click();
  await expect(page.getByRole("complementary", { name: "Thread" })).toBeVisible();
}

test("replies go into a thread beside the channel", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Release today?");
  await composer.press("Enter");

  await openThreadOf(page, "Release today?");
  const thread = page.getByRole("complementary", { name: "Thread" });
  await thread.getByLabel("Reply in thread").fill("After lunch");
  await thread.getByLabel("Reply in thread").press("Enter");

  await expect(thread.getByRole("article").filter({ hasText: "After lunch" })).toBeVisible();
  const channel = page.getByRole("log", { name: "Messages in #general" });
  await expect(channel.getByRole("button", { name: /1 reply/ })).toBeVisible();
  await expect(channel.getByRole("article").filter({ hasText: "After lunch" })).toHaveCount(0);
});

test("new replies in a thread you follow show until you open it", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Who's on call?");
  await composer.press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "Who's on call?" })).toBeVisible();

  await page.evaluate(() =>
    window.fake.receiveReply(window.fake.memberNamed("Grace"), window.fake.messageWithText("Who's on call?"), "Me"),
  );
  const summary = page.getByRole("button", { name: /1 reply.*1 new/ });
  await expect(summary).toBeVisible();

  await summary.click();
  await expect(
    page.getByRole("complementary", { name: "Thread" }).getByRole("article").filter({ hasText: "Me" }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: /1 new/ })).toHaveCount(0);
});

test("you can stop and start following a thread", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Plans for Friday");
  await composer.press("Enter");
  await openThreadOf(page, "Plans for Friday");

  const thread = page.getByRole("complementary", { name: "Thread" });
  await thread.getByRole("button", { name: "Following" }).click();
  await expect(thread.getByRole("button", { name: "Follow", exact: true })).toBeVisible();
  await thread.getByRole("button", { name: "Follow", exact: true }).click();
  await expect(thread.getByRole("button", { name: "Following" })).toBeVisible();

  await thread.getByRole("button", { name: "Close thread" }).click();
  await expect(thread).toHaveCount(0);
});
