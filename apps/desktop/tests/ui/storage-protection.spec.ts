import { expect, type Page, test } from "@playwright/test";
import { failWith } from "./support";

type Protection = "locked" | "versioned" | "unprotected" | "unknown";

async function setProtection(page: Page, state: Protection) {
  await page.evaluate((next) => window.fake.setStorageProtection(next), state);
}

async function openStorage(page: Page) {
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: /^Storage/ }).click();
  return settings.getByRole("tabpanel");
}

function meButton(page: Page) {
  return page.getByRole("button", { name: /^Ada\b/ });
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a locked bucket shows a quiet protected line", async ({ page }) => {
  await setProtection(page, "locked");
  const storage = await openStorage(page);

  await expect(storage).toContainText("Protected");
  await expect(storage).toContainText("Last checked");
  await expect(storage).not.toContainText("can lose data");
  await expect(storage.getByText("How to protect your bucket")).toHaveCount(0);
});

test("a versioned bucket shows a notice that can't be dismissed", async ({ page }) => {
  await setProtection(page, "versioned");
  const storage = await openStorage(page);

  const notice = storage.getByRole("region", { name: "Storage protection" });
  await expect(notice).toContainText("Accidents can be undone");
  await expect(notice).toContainText("S3 Object Lock, you can ignore this");
  await expect(notice.getByRole("button", { name: /dismiss|close/i })).toHaveCount(0);
  await expect(meButton(page).getByRole("img", { name: "Storage needs attention" })).toHaveCount(0);
});

test("an unprotected bucket warns, puts the retention lock first and marks the way there", async ({ page }) => {
  await setProtection(page, "unprotected");
  await expect(meButton(page).getByRole("img", { name: "Storage needs attention" })).toBeVisible();

  const storage = await openStorage(page);
  const notice = storage.getByRole("region", { name: "Storage protection" });
  await expect(notice).toContainText("Your storage can lose data");
  await expect(notice).toContainText("nothing could bring it back");
  const text = (await notice.textContent()) ?? "";
  expect(text.indexOf("retention lock")).toBeGreaterThan(-1);
  expect(text.indexOf("retention lock")).toBeLessThan(text.indexOf("versioning"));
  await expect(
    page
      .getByRole("dialog", { name: "Settings" })
      .getByRole("tab", { name: /^Storage/ })
      .getByRole("img", { name: "Storage needs attention" }),
  ).toBeVisible();
});

test("the dot goes away once the bucket is protected", async ({ page }) => {
  await setProtection(page, "unprotected");
  const dot = meButton(page).getByRole("img", { name: "Storage needs attention" });
  await expect(dot).toBeVisible();

  await setProtection(page, "locked");
  await expect(dot).toHaveCount(0);
});

test("an unchecked bucket says so quietly", async ({ page }) => {
  const storage = await openStorage(page);

  await expect(storage).toContainText("Couldn't check yet");
  await expect(storage).not.toContainText("can lose data");
});

test("checking again shows it's busy, then when it last checked", async ({ page }) => {
  const storage = await openStorage(page);
  await page.evaluate(() => {
    const answer = window.fake.handle.bind(window.fake);
    window.fake.handle = (command, args) =>
      command === "check_storage_protection"
        ? new Promise((resolve) => setTimeout(() => resolve(answer(command, args)), 400))
        : answer(command, args);
  });
  const checkAgain = storage.getByRole("button", { name: "Check again" });
  await checkAgain.click();

  await expect(checkAgain).toBeDisabled();
  await expect(checkAgain).toBeEnabled();
  await expect(storage).toContainText("Last checked");
});

test("a failed check says why, next to the button", async ({ page }) => {
  const storage = await openStorage(page);
  await failWith(page, "check_storage_protection", "the bucket can't be reached");
  await storage.getByRole("button", { name: "Check again" }).click();

  await expect(storage.getByRole("alert")).toHaveText("the bucket can't be reached");
});

test("the provider steps open from the keyboard and warn against locking everything", async ({ page }) => {
  await setProtection(page, "unprotected");
  const storage = await openStorage(page);
  const steps = storage.getByText("Bucket lock rules");
  await expect(steps).toBeHidden();

  await storage.getByText("How to protect your bucket").focus();
  await page.keyboard.press("Enter");
  await expect(steps).toBeVisible();
  await expect(storage).toContainText("mc version enable");
  await expect(storage).toContainText("Don't lock the whole bucket");
});

test.describe("as a member", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("?start=member");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("members see the warning and who fixes it", async ({ page }) => {
    await page.evaluate(() => window.fake.setStorageProtection("unprotected"));
    const storage = await openStorage(page);

    await expect(storage.getByRole("region", { name: "Storage protection" })).toContainText(
      "Your storage can lose data",
    );
    await expect(storage).toContainText("Ask Ada to fix this");
    await expect(storage).toContainText("Managed by Ada.");
  });
});
