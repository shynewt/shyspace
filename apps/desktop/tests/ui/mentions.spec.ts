import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    window.fake.addMember("Grace");
    window.fake.addMember("Gregor", { kind: "bot" });
  });
  await page.getByRole("button", { name: "general", exact: true }).click();
});

test("typing @ suggests people, and the picked name becomes a mention", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.pressSequentially("Thanks @Gr");
  const suggestions = page.getByRole("listbox", { name: "People to mention" });
  await expect(suggestions.getByRole("option")).toHaveCount(2);
  await composer.press("ArrowDown");
  await composer.press("ArrowUp");
  await composer.press("Enter");
  await expect(composer).toHaveValue("Thanks @Grace ");
  await composer.pressSequentially("for the review");
  await composer.press("Enter");

  const message = page.getByRole("article").filter({ hasText: "for the review" });
  await expect(message.locator(".mention", { hasText: "@Grace" })).toBeVisible();
});

test("@channel becomes a mention without picking", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("@channel standup now");
  await composer.press("Enter");

  await expect(page.getByRole("article").locator(".mention", { hasText: "@channel" })).toBeVisible();
});

test("a mention of you in another channel shows a red badge and stands out", async ({ page }) => {
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #random")).toBeVisible();
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.evaluate(() => {
    const me = window.fake.session?.me ?? "";
    window.fake.receiveMessage(window.fake.memberNamed("Grace"), "random", `[@Ada](shy://member/${me}) look at this`);
  });

  const random = page.getByRole("button", { name: "random, 1 unread, 1 mentions you" });
  await expect(random).toBeVisible();
  await random.click();
  await expect(page.locator(".mention-me", { hasText: "@Ada" })).toBeVisible();
});
