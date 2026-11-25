import { expect, type Page, test } from "@playwright/test";
import type { NotificationPermission } from "../../src/lib/api";

/** Starts with the system's answer (macOS) and what the person picks when asked. */
async function openWith(page: Page, permission: NotificationPermission, answerWhenAsked: NotificationPermission) {
  await page.addInitScript(
    ([permission, answerWhenAsked]) => {
      window.prepareFake = (fake) => {
        fake.notifications.permission = permission;
        fake.notifications.answerWhenAsked = answerWhenAsked;
      };
    },
    [permission, answerWhenAsked] as const,
  );
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
}

async function openNotificationSettings(page: Page) {
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Notifications" }).click();
  return settings;
}

const requests = (page: Page) => page.evaluate(() => window.fake.notifications.permissionRequests);

test("macOS asks once, when a workspace is open", async ({ page }) => {
  await openWith(page, "not_asked", "allowed");

  await expect.poll(() => requests(page)).toBe(1);
  const settings = await openNotificationSettings(page);
  await expect(settings.getByText("Notifications are off")).toHaveCount(0);
  expect(await requests(page)).toBe(1);
});

test("a dismissed prompt can be shown again from Settings", async ({ page }) => {
  await openWith(page, "not_asked", "not_asked");
  await expect.poll(() => requests(page)).toBe(1);

  const settings = await openNotificationSettings(page);
  await expect(settings.getByText("Notifications are off")).toBeVisible();
  await page.evaluate(() => {
    window.fake.notifications.answerWhenAsked = "allowed";
  });
  await settings.getByRole("button", { name: "Turn on notifications" }).click();

  expect(await requests(page)).toBe(2);
  await expect(settings.getByText("Notifications are off")).toHaveCount(0);
});

test("when macOS blocks notifications, Settings says so and opens System Settings", async ({ page }) => {
  await openWith(page, "denied", "denied");
  const settings = await openNotificationSettings(page);

  await expect(settings.getByText("macOS is blocking shyspace's notifications")).toBeVisible();
  await settings.getByRole("button", { name: "Open System Settings" }).click();
  expect(await page.evaluate(() => window.fake.notifications.settingsOpened)).toBe(1);

  await page.evaluate(() => {
    window.fake.notifications.permission = "allowed";
    window.dispatchEvent(new Event("focus"));
  });
  await expect(settings.getByText("macOS is blocking shyspace's notifications")).toHaveCount(0);
  expect(await requests(page)).toBe(0);
});

test("where notifications are always allowed, Settings shows nothing about permission", async ({ page }) => {
  await openWith(page, "allowed", "allowed");
  const settings = await openNotificationSettings(page);

  await expect(settings.getByText("Message text in notifications")).toBeVisible();
  await expect(settings.getByText("Notifications are off")).toHaveCount(0);
  await expect(settings.getByText("macOS is blocking shyspace's notifications")).toHaveCount(0);
});
