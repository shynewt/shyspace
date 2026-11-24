import { expect, type Page, test } from "@playwright/test";

/** Settings › Storage; the tab, to see its sign, and the panel. */
async function openStorage(page: Page) {
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  const tab = settings.getByRole("tab", { name: /^Storage/ });
  await tab.click();
  return { tab, panel: settings.getByRole("tabpanel") };
}

/** Grace is removed while Linus stays; returns nothing, the change is in the fake. */
async function removeGrace(page: Page) {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.addMember("Linus");
    window.fake.handle("remove_member", { member: grace });
  });
}

function card(panel: ReturnType<Page["getByRole"]>) {
  return panel.getByRole("region", { name: "Change the storage key" });
}

async function saveNewKey(panel: ReturnType<Page["getByRole"]>) {
  await panel.getByLabel("Access key").fill("NEWKEY");
  await panel.getByLabel("Secret key").fill("new-secret");
  await panel.getByRole("button", { name: "Save storage settings" }).click();
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("removing a member walks the admin through changing the storage key", async ({ page }) => {
  await removeGrace(page);
  const { tab, panel } = await openStorage(page);
  await expect(tab).toHaveAccessibleName(/Change the storage key/);
  const rotation = card(panel);
  await expect(rotation).toContainText("still has the storage key");
  await expect(rotation).toContainText("R2 object storage › Account Details");
  await expect(rotation.getByRole("link", { name: "Cloudflare's guide" })).toBeVisible();
  await expect(rotation.getByRole("button", { name: "I deleted the old key" })).toBeDisabled();

  await saveNewKey(panel);
  await expect(page.getByRole("dialog", { name: "Save your updated recovery kit" })).toContainText(
    "the old one stops working once you delete the old key",
  );
  await page.getByRole("button", { name: "I saved it" }).click();

  const moves = rotation.getByRole("list", { name: "Moved to the new key" });
  await expect(moves.getByRole("listitem").filter({ hasText: "Linus" })).toContainText("Not yet");
  await expect(rotation).toContainText("Deleting the old key now cuts off 1 person who hasn't moved yet");
  await page.evaluate(() => window.fake.memberMovesToNewKey("Linus"));
  await expect(moves.getByRole("listitem").filter({ hasText: "Linus" })).toContainText("Moved");
  await expect(rotation).toContainText("Everyone has moved");

  await page.evaluate(() => {
    window.fake.oldKeyCheck = "still_works";
  });
  await rotation.getByRole("button", { name: "I deleted the old key" }).click();
  await expect(rotation.getByRole("alert")).toContainText("The old key still works");

  await page.evaluate(() => {
    window.fake.oldKeyCheck = "dead";
  });
  await rotation.getByRole("button", { name: "Check again" }).click();
  await expect(rotation).toHaveCount(0);
  await expect(
    page.getByText("The old key is deleted. Removed members can't reach the bucket any more."),
  ).toBeVisible();
  await expect(tab).not.toHaveAccessibleName(/Change the storage key/);
});

test("a check that can't reach the bucket says it will try again by itself", async ({ page }) => {
  await removeGrace(page);
  const { panel } = await openStorage(page);
  await saveNewKey(panel);
  await page.getByRole("button", { name: "I saved it" }).click();
  await page.evaluate(() => {
    window.fake.oldKeyCheck = "unknown";
  });
  await card(panel).getByRole("button", { name: "I deleted the old key" }).click();
  await expect(card(panel)).toContainText("Couldn't reach the bucket to check. The app tries again every 10 minutes.");
});

test("stopping asks first, and says the removed member's key may still work", async ({ page }) => {
  await removeGrace(page);
  const { tab, panel } = await openStorage(page);
  await card(panel).getByRole("button", { name: "Don't change the key" }).click();
  const confirm = page.getByRole("dialog", { name: "Stop changing the storage key?" });
  await expect(confirm).toContainText("The removed member's key may still work");
  await confirm.getByRole("button", { name: "Stop" }).click();
  await expect(card(panel)).toHaveCount(0);
  await expect(tab).not.toHaveAccessibleName(/Change the storage key/);
});

test("members never see the key change", async ({ page }) => {
  await page.goto("?start=member");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("tab", { name: "Profile" })).toBeVisible();
  await expect(settings.getByRole("tab", { name: /^Storage/ })).toHaveCount(0);
  await expect(page.getByRole("status").filter({ hasText: "only the admin" })).toHaveCount(0);
});
