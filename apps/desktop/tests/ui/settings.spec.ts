import { expect, type Page, test } from "@playwright/test";
import { failWith } from "./support";

async function openSettings(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+,");
  return page.getByRole("dialog", { name: "Settings" });
}

test("you can change the name everyone sees", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByLabel("Display name").fill("Ada Lovelace");
  await settings.getByRole("button", { name: "Save profile" }).click();

  await expect(page.getByText("Your name is updated for everyone.")).toBeVisible();
  expect(await page.evaluate(() => window.fake.handle("members", undefined))).toContainEqual(
    expect.objectContaining({ name: "Ada Lovelace", is_admin: true }),
  );
});

test("the theme can be chosen and kept", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("radio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
});

test("notification text can be hidden", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Notifications" }).click();
  await settings.getByRole("radio", { name: "Hide text" }).click();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #random")).toBeVisible();
  await page.getByRole("button", { name: "general", exact: true }).click();

  await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "random", "secret plans"));
  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown))
    .toEqual([{ title: "Grace in #random", body: "New message" }]);
});

test("the arrow keys move between choices", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  const theme = settings.getByRole("radiogroup", { name: "Theme" });
  await theme.getByRole("radio", { name: "System" }).focus();

  await page.keyboard.press("ArrowRight");
  await expect(theme.getByRole("radio", { name: "Light" })).toBeChecked();
  await expect(theme.getByRole("radio", { name: "Light" })).toBeFocused();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
});

test("settings fit the smallest window and scroll inside it", async ({ page }) => {
  await page.setViewportSize({ width: 820, height: 560 });
  const settings = await openSettings(page);
  await expect(settings).toBeInViewport({ ratio: 1 });
  await page.evaluate(() => {
    for (let index = 1; index <= 12; index++) window.fake.addMember(`Member ${index}`);
  });

  await settings.getByRole("tab", { name: "Members" }).first().click();
  const lastMember = settings.getByRole("listitem").filter({ hasText: "Member 12" });
  await lastMember.scrollIntoViewIfNeeded();
  await expect(lastMember).toBeInViewport({ ratio: 1 });
  await expect(settings).toBeInViewport({ ratio: 1 });
});

test("the close button closes settings", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("button", { name: "Close" }).click();
  await expect(settings).toBeHidden();
});

test("settings group your sections and the workspace's", async ({ page }) => {
  const settings = await openSettings(page);
  const sections = settings.getByRole("tablist", { name: "Settings sections" });
  await expect(sections.getByRole("tab")).toHaveText([
    "Profile",
    "Appearance",
    "Notifications",
    "Privacy & security",
    "Members",
    "Agents",
    "Storage",
    "Recovery",
  ]);
  await expect(sections.getByText("You", { exact: true })).toBeVisible();
  await expect(sections.getByText("Acme", { exact: true })).toBeVisible();
});

test("the arrow keys move between sections", async ({ page }) => {
  const settings = await openSettings(page);
  await expect(settings.getByRole("tab", { name: "Profile" })).toBeFocused();

  await page.keyboard.press("ArrowDown");
  await expect(settings.getByRole("tab", { name: "Appearance" })).toHaveAttribute("aria-selected", "true");
  await expect(settings.getByRole("radiogroup", { name: "Theme" })).toBeVisible();
});

test("settings keep their size from section to section", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  const settings = await openSettings(page);
  const profileBox = await settings.boundingBox();

  await settings.getByRole("tab", { name: "Appearance" }).click();
  await expect(settings.getByRole("radiogroup", { name: "Theme" })).toBeVisible();
  expect(await settings.boundingBox()).toEqual(profileBox);
});

test("the palette opens settings at a section", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("notifications");
  await page.getByRole("option", { name: "Settings: Notifications" }).click();

  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("tab", { name: "Notifications" })).toHaveAttribute("aria-selected", "true");
  await expect(settings.getByRole("tab", { name: "Notifications" })).toBeFocused();
});

test("privacy explains the encryption, with details on request", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Privacy & security" }).click();
  await expect(settings.getByText(/encrypted on this device/)).toBeVisible();
  const details = settings.getByText(/system's keychain/);
  await expect(details).toBeHidden();

  await settings.getByText("How it works").click();
  await expect(details).toBeVisible();
});

test("a failed profile save keeps the new name and says why", async ({ page }) => {
  const settings = await openSettings(page);
  await failWith(page, "set_profile", "the workspace is offline");
  await settings.getByLabel("Display name").fill("Ada Lovelace");
  await settings.getByRole("button", { name: "Save profile" }).click();

  await expect(settings.getByRole("alert")).toHaveText("the workspace is offline");
  await expect(settings.getByLabel("Display name")).toHaveValue("Ada Lovelace");
});

test("the admin can change the storage keys", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Storage" }).click();
  await expect(settings.getByText("https://s3.example.com")).toBeVisible();
  await settings.getByLabel("Access key").fill("AKIDNEW");
  await settings.getByLabel("Secret key").fill("new-secret");
  await settings.getByRole("button", { name: "Save storage settings" }).click();

  await expect(page.getByRole("dialog", { name: "Save your updated recovery kit" })).toBeVisible();
  expect(await page.evaluate(() => window.fake.handle("storage_settings", undefined))).toMatchObject({
    access_key_id: "AKIDNEW",
  });
});

test("a failed storage save keeps what you typed and says why", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Storage" }).click();
  await failWith(page, "update_storage", "the bucket refused these keys");
  await settings.getByLabel("Access key").fill("AKIDNEW");
  await settings.getByLabel("Secret key").fill("wrong-secret");
  await settings.getByRole("button", { name: "Save storage settings" }).click();

  await expect(settings.getByRole("alert")).toHaveText("the bucket refused these keys");
  await expect(settings.getByLabel("Access key")).toHaveValue("AKIDNEW");
  await expect(settings.getByLabel("Secret key")).toHaveValue("wrong-secret");
});

test("the admin can show the recovery kit", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Recovery" }).click();
  await settings.getByRole("button", { name: "Show recovery kit" }).click();

  await expect(page.getByRole("dialog", { name: "Your recovery kit" })).toContainText(/shyrec1_/);
});

test("members see who manages storage and recovery", async ({ page }) => {
  await page.goto("?start=member");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });

  await settings.getByRole("tab", { name: "Storage" }).click();
  await expect(settings.getByRole("tabpanel")).toContainText("Managed by Ada.");
  await expect(settings.getByLabel("Secret key")).toHaveCount(0);
  await settings.getByRole("tab", { name: "Recovery" }).click();
  await expect(settings.getByRole("tabpanel")).toContainText("Managed by Ada.");
  await expect(settings.getByRole("button", { name: "Show recovery kit" })).toHaveCount(0);
});

/** The cheat sheet's transform in each frame while it opens: "none" throughout when motion is reduced. */
async function transformsWhileOpeningADialog(page: Page): Promise<string[]> {
  return page.evaluate(async () => {
    const seen = new Set<string>();
    const sampled = new Promise<void>((resolve) => {
      const start = performance.now();
      const sample = () => {
        const dialog = document.querySelector('[role="dialog"]');
        if (dialog) seen.add(getComputedStyle(dialog).transform);
        if (performance.now() - start < 300) requestAnimationFrame(sample);
        else resolve();
      };
      requestAnimationFrame(sample);
    });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "/", ctrlKey: true }));
    await sampled;
    return [...seen];
  });
}

test("reduce motion can be turned on whatever the system says", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("radiogroup", { name: "Reduce motion" }).getByRole("radio", { name: "On" }).click();
  await page.keyboard.press("Escape");
  await expect(settings).toBeHidden();

  expect(await transformsWhileOpeningADialog(page)).toEqual(["none"]);
});

test("reduce motion can be turned off when the system asks for less", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("radiogroup", { name: "Reduce motion" }).getByRole("radio", { name: "Off" }).click();
  await page.keyboard.press("Escape");
  await expect(settings).toBeHidden();

  const transforms = await transformsWhileOpeningADialog(page);
  expect(transforms.some((transform) => transform.startsWith("matrix("))).toBe(true);
});

test("typed text waits while you look at another section", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByLabel("Display name").fill("Ada Lovelace");
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("tab", { name: "Profile" }).click();

  await expect(settings.getByLabel("Display name")).toHaveValue("Ada Lovelace");
});

test("a storage save that ends on another section still shows the new kit", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Storage" }).click();
  await page.evaluate(() => {
    const answer = window.fake.handle.bind(window.fake);
    window.fake.handle = (command, args) =>
      command === "update_storage"
        ? new Promise((resolve) => setTimeout(() => resolve(answer(command, args)), 400))
        : answer(command, args);
  });
  await settings.getByLabel("Secret key").fill("new-secret");
  await settings.getByRole("button", { name: "Save storage settings" }).click();
  await settings.getByRole("tab", { name: "Appearance" }).click();

  await expect(page.getByRole("dialog", { name: "Save your updated recovery kit" })).toBeVisible();
});
