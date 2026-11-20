import { expect, type Page, test } from "@playwright/test";

async function openSettings(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+,");
  return page.getByRole("dialog", { name: "Settings" });
}

test("you can change the name everyone sees", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByLabel("Display name").fill("Ada Lovelace");
  await settings.getByRole("button", { name: "Save" }).click();

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
