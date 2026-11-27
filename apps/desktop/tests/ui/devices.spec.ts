import { expect, type Page, test } from "@playwright/test";

async function openDevices(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Devices" }).click();
  return settings;
}

async function openLinkDialog(page: Page) {
  const settings = await openDevices(page);
  await settings.getByRole("button", { name: "Link a device" }).click();
  return page.getByRole("dialog", { name: "Link a device" });
}

test("your devices list this one first, with nothing to remove when it's your only one", async ({ page }) => {
  const settings = await openDevices(page);

  await expect(settings.getByText("This device", { exact: true })).toBeVisible();
  await expect(settings.getByText("Active now", { exact: true })).toBeVisible();
  await expect(settings.getByRole("button", { name: "Remove this device" })).toHaveCount(0);
});

test("linking shows a code to scan or copy, and what the new device will see", async ({ page }) => {
  const dialog = await openLinkDialog(page);

  await expect(dialog.getByRole("img", { name: "The link code, to scan on your new device" })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Copy the link code" })).toBeVisible();
  await expect(dialog.getByText("not private channels or DMs yet")).toBeVisible();
});

test("picking the new device's number links it, and it shows as linking", async ({ page }) => {
  const dialog = await openLinkDialog(page);
  await page.evaluate(() => window.fake.devices.newDeviceAsks());

  await expect(dialog.getByRole("heading", { name: "Tap the number your new device shows" })).toBeFocused();
  const choices = dialog.getByRole("group", { name: "Tap the number your new device shows" });
  await expect(choices.getByRole("button")).toHaveCount(3);
  await choices.getByRole("button", { name: "482, 917" }).click();

  await expect(dialog.getByText("Linked. Your new device can read now")).toBeVisible();
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(page.getByText("Linking… It can send once your workspace adds it.")).toBeVisible();
});

test("a wrong number ends the link, and a new code starts again", async ({ page }) => {
  const dialog = await openLinkDialog(page);
  await page.evaluate(() => window.fake.devices.newDeviceAsks());
  const choices = dialog.getByRole("group", { name: "Tap the number your new device shows" });

  await choices.getByRole("button", { name: "305, 116" }).click();

  await expect(dialog.getByText("That wasn't the number. Start again with a new code.")).toBeVisible();
  await dialog.getByRole("button", { name: "New code" }).click();
  await expect(dialog.getByRole("button", { name: "Copy the link code" })).toBeVisible();
});

test("none of these stops the link", async ({ page }) => {
  const dialog = await openLinkDialog(page);
  await page.evaluate(() => window.fake.devices.newDeviceAsks());

  await dialog.getByRole("button", { name: "None of these" }).click();

  await expect(dialog.getByText("Stopped. Nothing was shared.")).toBeVisible();
  expect(await page.evaluate(() => window.fake.devices.hasOpenCode)).toBe(false);
});

test("closing the dialog stops the link", async ({ page }) => {
  const dialog = await openLinkDialog(page);
  await expect(dialog.getByRole("button", { name: "Copy the link code" })).toBeVisible();

  await dialog.getByRole("button", { name: "Close" }).click();

  await expect(dialog).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.fake.devices.hasOpenCode)).toBe(false);
});

test("removing a device that linked others offers removing only it, or them too", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() =>
    window.fake.devices.setDevices([
      { id: "a".repeat(64), platform: "Android", linked_ms: Date.UTC(2026, 11, 3), linked_devices: ["b".repeat(64)] },
      { id: "b".repeat(64), platform: "Linux", linked_ms: Date.UTC(2026, 11, 4), has_no_activity_seen: true },
    ]),
  );
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Devices" }).click();

  await settings.getByRole("button", { name: /Remove Android, linked/ }).click();

  const confirm = page.getByRole("dialog", { name: /Remove "Android, linked/ });
  await expect(confirm.getByText(/It linked: Linux, linked/)).toBeVisible();
  await expect(confirm.getByRole("button", { name: "Also remove the devices it linked" })).toBeVisible();
  await confirm.getByRole("button", { name: "Remove this one" }).click();
  await expect(settings.getByText("Removing… finishes when your workspace confirms.")).toBeVisible();
});

test("a device linked from another of yours shows in the inbox until you've seen it", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() =>
    window.fake.devices.setNotices([
      { device: "c".repeat(64), kind: "linked", platform: "Android", at_ms: Date.now(), is_seen: false },
    ]),
  );

  await page.getByRole("button", { name: "Inbox", exact: true }).click();
  const section = page.getByRole("region", { name: "Your devices" });
  await expect(section.getByText("A new device can use shyspace as you")).toBeVisible();
  await expect(section.getByText(/Android, linked from another of your devices/)).toBeVisible();

  await section.getByRole("button", { name: "Got it" }).click();
  await expect(section).toHaveCount(0);
  expect(await page.evaluate(() => window.fake.devices.seenNotices)).toBe(true);
});

test("review devices opens Settings at Devices", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() =>
    window.fake.devices.setNotices([
      { device: "c".repeat(64), kind: "removed", platform: null, at_ms: Date.now(), is_seen: false },
    ]),
  );
  await page.getByRole("button", { name: "Inbox", exact: true }).click();

  await page.getByRole("button", { name: "Review devices" }).click();

  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("tab", { name: "Devices" })).toHaveAttribute("aria-selected", "true");
});

test("a notice arriving while the app runs shows a desktop notification; older ones don't", async ({ page }) => {
  await page.addInitScript(() => {
    window.prepareFake = (fake) => {
      fake.devices.setNotices([
        { device: "d".repeat(64), kind: "removed", platform: "Linux", at_ms: Date.UTC(2026, 10, 20), is_seen: false },
      ]);
    };
  });
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.evaluate(() =>
    window.fake.devices.setNotices([
      { device: "d".repeat(64), kind: "removed", platform: "Linux", at_ms: Date.UTC(2026, 10, 20), is_seen: false },
      { device: "e".repeat(64), kind: "linked", platform: "Android", at_ms: Date.now(), is_seen: false },
    ]),
  );

  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown.map((alert) => alert.title)))
    .toEqual(["A new device can use shyspace as you"]);
});

test("a notice in a workspace that isn't shown alerts too", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  const team = await page.evaluate(() => window.fake.workspaces.add("Team"));
  await expect(page.getByRole("button", { name: /^Team/ })).toBeVisible();

  await page.evaluate(
    (id) =>
      window.fake.devices.setNoticesIn(id, [
        { device: "f".repeat(64), kind: "linked", platform: "Android", at_ms: Date.UTC(2026, 10, 27), is_seen: false },
      ]),
    team,
  );

  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown.map((alert) => alert.title)))
    .toEqual(["A new device can use shyspace as you"]);
  expect(await page.evaluate(() => window.fake.alertWorkspaces)).toEqual([team]);
});
