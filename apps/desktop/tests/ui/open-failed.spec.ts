import { expect, test } from "@playwright/test";

test("a workspace that won't open says why, instead of offering to start over", async ({ page }) => {
  await page.addInitScript(() => {
    window.prepareFake = (fake) => {
      fake.sessionFailure = "couldn't open your workspace: it's already open in another shyspace window";
    };
  });
  await page.goto("");

  await expect(page.getByRole("heading", { name: "shyspace couldn't open your workspace" })).toBeVisible();
  await expect(page.getByText("it's already open in another shyspace window")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Welcome to shyspace" })).toHaveCount(0);

  await page.evaluate(() => {
    window.fake.sessionFailure = null;
  });
  await page.getByRole("button", { name: "Try again" }).click();

  await expect(page.getByLabel("Message #general")).toBeVisible();
});
