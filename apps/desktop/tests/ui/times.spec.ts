import { expect, test } from "@playwright/test";

test.use({ locale: "en-US", timezoneId: "UTC" });

const YESTERDAY_AFTERNOON = Date.UTC(2026, 10, 19, 16, 41);
const TODAY_MORNING = Date.UTC(2026, 10, 20, 9, 5);

test.beforeEach(async ({ page }) => {
  await page.clock.setFixedTime(YESTERDAY_AFTERNOON);
  await page.goto("");
  await page.getByRole("button", { name: "Inbox", exact: true }).click();
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", `[@Ada](shy://member/${window.fake.session?.me}) can you look?`);
    const question = window.fake.receiveMessage(grace, "general", "Who's on call?");
    window.fake.receiveReply(grace, question, "Me");
  });
  await expect(page.getByRole("region", { name: "Mentions and replies" })).toBeVisible();
  await page.clock.setFixedTime(TODAY_MORNING);
});

test("older times say which day", async ({ page }) => {
  await page.getByRole("button", { name: "Later", exact: true }).click();
  await page.getByRole("button", { name: /^Inbox/ }).click();
  await expect(page.getByRole("region", { name: "Mentions and replies" })).toContainText("Yesterday 4:41 PM");

  await page
    .getByRole("navigation", { name: "Workspace" })
    .getByRole("button", { name: /^general/ })
    .click();
  await expect(page.getByRole("button", { name: /1 reply/ })).toContainText("Last reply Yesterday 4:41 PM");
});

test("message times drop the leading zero and show the full date on hover", async ({ page }) => {
  await page
    .getByRole("navigation", { name: "Workspace" })
    .getByRole("button", { name: /^general/ })
    .click();
  const composer = page.getByLabel("Message #general");
  await composer.fill("Morning");
  await composer.press("Enter");

  const message = page.getByRole("article").filter({ hasText: "Morning" });
  const time = message.getByText("9:05 AM", { exact: true });
  await expect(time).toBeVisible();
  await time.hover();
  await expect(page.getByRole("tooltip")).toContainText("Friday, November 20, 2026 at 9:05 AM");
  await expect(message).toHaveAccessibleName("Ada, Friday, November 20, 2026 at 9:05 AM");
});
