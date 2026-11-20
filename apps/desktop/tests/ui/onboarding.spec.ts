import { expect, type Page, test } from "@playwright/test";

async function fillBucket(page: Page, secret: string) {
  await page.getByLabel("Workspace").fill("Acme");
  await page.getByLabel("Your name").fill("Ada");
  await page.getByLabel("Endpoint").fill("https://s3.example.com");
  await page.getByLabel("Bucket").fill("team");
  await page.getByLabel("Region").fill("auto");
  await page.getByLabel("Access key").fill("AKIDEXAMPLE");
  await page.getByLabel("Secret key").fill(secret);
}

test.beforeEach(async ({ page }) => {
  await page.goto("?start=onboarding");
  await expect(page.getByRole("heading", { name: "Welcome to shyspace" })).toBeVisible();
});

test("creating a workspace needs a bucket that passed the connection test", async ({ page }) => {
  const create = page.getByRole("button", { name: "Create workspace" });
  await fillBucket(page, "wrong-secret");
  await expect(create).toBeDisabled();

  await page.getByRole("button", { name: "Test connection" }).click();
  await expect(page.getByText(/refused these keys/)).toBeVisible();
  await expect(create).toBeDisabled();

  await page.getByLabel("Secret key").fill("right-secret");
  await page.getByRole("button", { name: "Test connection" }).click();
  await expect(page.getByText("The bucket works")).toBeVisible();
  await create.click();

  await expect(page.getByRole("button", { name: "general", exact: true })).toBeVisible();
});

test("changing the settings after a passed test asks for a new test", async ({ page }) => {
  await fillBucket(page, "right-secret");
  await page.getByRole("button", { name: "Test connection" }).click();
  await expect(page.getByText("The bucket works")).toBeVisible();

  await page.getByLabel("Bucket").fill("another-bucket");
  await expect(page.getByText("Settings changed; test again.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Create workspace" })).toBeDisabled();
});

test("joining with an invite opens the workspace once approved", async ({ page }) => {
  await page.getByRole("radio", { name: "Join" }).click();
  await page.getByLabel("Invite code").fill("shy2_example");
  await page.getByLabel("Your name").fill("Grace");
  await page.getByRole("button", { name: /Ask to join/ }).click();

  await expect(page.getByText("Team")).toBeVisible();
  await expect(page.getByRole("button", { name: "general", exact: true })).toBeVisible();
});

test("a join request can be cancelled while waiting, keeping what you typed", async ({ page }) => {
  await page.evaluate(() => window.fake.holdJoins());
  await page.getByRole("radio", { name: "Join" }).click();
  await page.getByLabel("Invite code").fill("shy2_first");
  await page.getByLabel("Your name").fill("Grace");
  await page.getByRole("button", { name: /Ask to join/ }).click();
  await expect(page.getByText("Waiting for the admin…")).toBeVisible();

  await page.getByRole("button", { name: "Cancel request" }).click();

  await expect(page.getByLabel("Invite code")).toHaveValue("shy2_first");
  await expect(page.getByLabel("Your name")).toHaveValue("Grace");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await page.getByLabel("Invite code").fill("shy2_second");
  await page.getByRole("button", { name: /Ask to join/ }).click();
  await page.evaluate(() => window.fake.approveJoin());
  await expect(page.getByRole("button", { name: "general", exact: true })).toBeVisible();
});
