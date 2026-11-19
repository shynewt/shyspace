import { expect, test } from "@playwright/test";

test("a waiting join request shows a badge until it is approved", async ({ page }) => {
  await page.goto("");
  const membersButton = page.getByRole("button", { name: /Members & invites/ });
  await expect(membersButton.getByLabel(/waiting/)).toHaveCount(0);

  await page.evaluate(() => window.fake.receiveJoinRequest("Claude", { kind: "bot" }));
  await expect(membersButton.getByLabel("1 join request waiting")).toBeVisible();

  await membersButton.click();
  await expect(page.getByText("1234 5678 9012 3456 7890")).toBeVisible();
  await expect(page.getByText(/end-to-end encryption ends there/)).toBeVisible();
  await page.getByRole("button", { name: "Codes match, let in" }).click();

  await expect(page.getByText("Claude wants to join")).toHaveCount(0);
  await expect(page.getByRole("dialog").getByText("Claude", { exact: true })).toBeVisible();
  await expect(membersButton.getByLabel(/waiting/)).toHaveCount(0);
});

test("creating an invite shows a code to copy", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: /Members & invites/ }).click();
  await page.getByLabel("Who the invite is for").fill("Grace");
  await page.getByRole("button", { name: "Create", exact: true }).click();

  await expect(page.getByRole("button", { name: "Copy invite code" })).toBeVisible();
  await expect(page.getByText(/^shy2_/)).toBeVisible();
});
