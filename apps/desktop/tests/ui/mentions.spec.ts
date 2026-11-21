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
  const suggestions = page.getByRole("listbox", { name: "People and pages to mention" });
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

test("a space after someone's whole name mentions them without picking", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.pressSequentially("@grace thanks");
  await expect(composer).toHaveValue("@Grace thanks");
  await composer.press("Enter");

  const message = page.getByRole("article").filter({ hasText: "thanks" });
  await expect(message.locator(".mention", { hasText: "@Grace" })).toBeVisible();
});

test("a whole name sent right away mentions them, and a part of one doesn't", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("@grace, thanks");
  await composer.press("Enter");
  await expect(
    page.getByRole("article").filter({ hasText: "thanks" }).locator(".mention", { hasText: "@Grace" }),
  ).toBeVisible();

  await composer.pressSequentially("@gra later");
  await expect(composer).toHaveValue("@gra later");
  await composer.press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "later" }).locator(".mention")).toHaveCount(0);
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

test("@ finds pages by any word of their title, and sends a page link", async ({ page }) => {
  const planId = await page.evaluate(() => {
    window.fake.seedPage("Growth ideas", "");
    return window.fake.seedPage("Launch plan", "");
  });
  const composer = page.getByLabel("Message #general");
  const suggestions = page.getByRole("listbox", { name: "People and pages to mention" });
  await composer.pressSequentially("See @");
  await expect(suggestions.getByRole("option", { name: /Launch plan|Growth ideas/ })).toHaveCount(0);

  await composer.pressSequentially("g");
  await expect(suggestions.getByRole("option")).toHaveText([/Grace/, /Gregor/, /Growth ideas/]);
  await expect(suggestions.getByText("Pages", { exact: true })).toBeVisible();

  await composer.press("Backspace");
  await composer.pressSequentially("pla");
  await expect(suggestions.getByRole("option")).toHaveText(["Launch plan"]);
  await composer.press("Enter");
  await expect(composer).toHaveValue("See @Launch plan ");
  await composer.pressSequentially("before Friday");
  await composer.press("Enter");

  const message = page.getByRole("article").filter({ hasText: "before Friday" });
  await expect(message.locator(".page-chip", { hasText: "Launch plan" })).toBeVisible();
  const sent = `See shy://page/${planId} before Friday`;
  expect(await page.evaluate((text) => window.fake.messageWithText(text), sent)).toBeTruthy();
});

test("dragging a page from the sidebar onto the message box mentions it", async ({ page }) => {
  await page.evaluate(() => window.fake.seedPage("Launch plan", ""));
  const composer = page.getByLabel("Message #general");
  await composer.fill("Have a look at");
  const row = page
    .getByRole("navigation", { name: "Workspace" })
    .getByRole("button", { name: "Launch plan", exact: true });
  await row.dragTo(composer);
  await expect(composer).toHaveValue("Have a look at @Launch plan ");
  await expect(composer).toBeFocused();
  await composer.press("Enter");

  const message = page.getByRole("article").filter({ hasText: "Have a look at" });
  await expect(message.locator(".page-chip", { hasText: "Launch plan" })).toBeVisible();
});

test("arrows held with Ctrl or Super don't move through the suggestions", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.pressSequentially("Thanks @Gr");
  const options = page.getByRole("listbox", { name: "People and pages to mention" }).getByRole("option");
  await expect(options).toHaveCount(2);
  await expect(options.first()).toHaveAttribute("aria-selected", "true");

  for (const key of ["Meta+ArrowDown", "Control+ArrowDown", "Meta+ArrowUp"]) await composer.press(key);

  await expect(options.first()).toHaveAttribute("aria-selected", "true");
});
