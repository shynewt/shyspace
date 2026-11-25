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

test("a workspace that keeps failing still leaves a way to set up another", async ({ page }) => {
  await page.addInitScript(() => {
    window.prepareFake = (fake) => {
      fake.sessionFailure = "couldn't open your workspace: this workspace was made by a different build";
    };
  });
  await page.goto("");
  await expect(page.getByText("won't work while it's still on this device")).toBeVisible();

  await page.getByRole("button", { name: "Set up another workspace" }).click();
  await expect(page.getByRole("heading", { name: "Add a workspace" })).toBeVisible();
  await page.getByRole("button", { name: "Back" }).click();
  await expect(page.getByText("this workspace was made by a different build")).toBeVisible();

  await page.getByRole("button", { name: "Set up another workspace" }).click();
  await page.getByLabel("Workspace").fill("Acme");
  await page.getByLabel("Your name").fill("Ada");
  await page.getByLabel("Endpoint").fill("https://s3.example.com");
  await page.getByLabel("Bucket").fill("team");
  await page.getByLabel("Region").fill("auto");
  await page.getByLabel("Access key").fill("AKIDEXAMPLE");
  await page.getByLabel("Secret key").fill("right-secret");
  await page.getByRole("button", { name: "Test connection" }).click();
  await expect(page.getByText("The bucket works")).toBeVisible();
  await page.getByRole("button", { name: "Create workspace" }).click();

  await expect(page.getByLabel("Message #general")).toBeVisible();
});
