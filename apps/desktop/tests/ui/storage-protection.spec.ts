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
  // A key without version deletes makes versioning enough, and the app can't see the key's rights.
  await expect(notice).toContainText("If the members' key can't delete old versions");
  await expect(notice).toContainText("shyspace can't check that from here");
  await expect(notice.getByRole("button", { name: /dismiss|close/i })).toHaveCount(0);
  await expect(meButton(page).getByRole("img", { name: "Storage needs attention" })).toHaveCount(0);
});

test("an unprotected bucket warns, says what to turn on and marks the way there", async ({ page }) => {
  await setProtection(page, "unprotected");
  await expect(meButton(page).getByRole("img", { name: "Storage needs attention" })).toBeVisible();

  const storage = await openStorage(page);
  const notice = storage.getByRole("region", { name: "Storage protection" });
  await expect(notice).toContainText("Your storage can lose data");
  await expect(notice).toContainText("nothing could bring it back");
  await expect(notice).toContainText("Turn on versioning, and give members a key that can't delete old versions");
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
  // MinIO only refuses a version delete when a statement denies it; leaving the action out isn't enough.
  await expect(storage).toContainText(/explicit Deny on s3:DeleteObjectVersion for \S+\/\*/);
  await expect(storage).toContainText("MinIO's open-source edition is no longer maintained");
  await expect(storage).toContainText("Don't lock the whole bucket");
  // A lock refuses overwrites, and membership repairs mls/ by overwriting.
  await expect(storage).toContainText(/Don't lock \S*mls\/: membership changes repair it by overwriting/);
  await expect(storage).not.toContainText(/rule for \S*ops\/ and one for/);
});

test.describe("as a member", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("?start=member");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("members aren't asked about storage they can't fix", async ({ page }) => {
    await page.evaluate(() => window.fake.setStorageProtection("unprotected"));
    const me = page.getByRole("button", { name: /^Grace/ });
    await expect(me).toBeVisible();
    await expect(me.getByRole("img", { name: "Storage needs attention" })).toHaveCount(0);
    await me.click();
    await expect(page.getByRole("menuitem", { name: "Storage needs attention" })).toHaveCount(0);
  });
});

test("the warning sign explains itself and the me menu leads to the fix", async ({ page }) => {
  await setProtection(page, "unprotected");
  const sign = meButton(page).getByRole("img", { name: "Storage needs attention" });
  await sign.hover();
  await expect(page.getByRole("tooltip", { name: "Storage can lose data" })).toBeVisible();

  await meButton(page).click();
  await page.getByRole("menuitem", { name: "Storage needs attention" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" }).getByRole("tab", { name: /^Storage/ })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("the me menu has no storage item while the bucket is protected", async ({ page }) => {
  await setProtection(page, "locked");
  await meButton(page).click();
  await expect(page.getByRole("menuitem", { name: "Set status" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Storage needs attention" })).toHaveCount(0);
});

test("the protection steps name this workspace's own folders in the bucket", async ({ page }) => {
  await setProtection(page, "unprotected");
  const storage = await openStorage(page);
  await storage.getByText("How to protect your bucket").click();
  // Rules match whole prefixes, and every object sits under the workspace id.
  await expect(storage.getByText("a1b2c3d4e5f60718/ops/").first()).toBeVisible();
  await expect(storage.getByText("a1b2c3d4e5f60718/files/").first()).toBeVisible();
  // An Object Lock default retention would freeze the whole bucket, files and recovery snapshot included.
  await expect(storage).not.toContainText("then set an Object Lock default retention");
  await expect(storage).toContainText("Don't set an Object Lock default retention");
});
