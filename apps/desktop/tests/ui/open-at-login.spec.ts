import { expect, type Page, test } from "@playwright/test";

/** Opens Settings › General, with the system's record of opening at login as given. */
async function openGeneral(page: Page, isOn: boolean) {
  await page.addInitScript((isOn) => {
    window.prepareFake = (fake) => {
      fake.openAtLogin.isOn = isOn;
    };
  }, isOn);
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "General" }).click();
  return settings;
}

const toggle = (settings: ReturnType<Page["getByRole"]>) =>
  settings.getByRole("switch", { name: "Open shyspace when you log in" });

test("opening at login is off until turned on, and turning it on sets the system's entry", async ({ page }) => {
  const settings = await openGeneral(page, false);
  await expect(toggle(settings)).not.toBeChecked();

  await toggle(settings).click();

  await expect(toggle(settings)).toBeChecked();
  expect(await page.evaluate(() => window.fake.openAtLogin.isOn)).toBe(true);
  await toggle(settings).click();
  await expect(toggle(settings)).not.toBeChecked();
  expect(await page.evaluate(() => window.fake.openAtLogin.isOn)).toBe(false);
});

test("the switch shows what the system says, also after a change made in its own settings", async ({ page }) => {
  const settings = await openGeneral(page, true);
  await expect(toggle(settings)).toBeChecked();

  await page.evaluate(() => {
    window.fake.openAtLogin.isOn = false;
    window.dispatchEvent(new Event("focus"));
  });

  await expect(toggle(settings)).not.toBeChecked();
});

test("when the system refuses, the switch goes back and says why", async ({ page }) => {
  const settings = await openGeneral(page, false);
  await page.evaluate(() => {
    window.fake.openAtLogin.refusal = "macOS needs your OK: allow shyspace in System Settings › General › Login Items";
  });

  await toggle(settings).click();

  await expect(settings.getByText("macOS needs your OK")).toBeVisible();
  await expect(toggle(settings)).not.toBeChecked();
  expect(await page.evaluate(() => window.fake.openAtLogin.isOn)).toBe(false);
});

test("coming back after allowing it elsewhere clears the old reason", async ({ page }) => {
  const settings = await openGeneral(page, false);
  await page.evaluate(() => {
    window.fake.openAtLogin.refusal = "macOS needs your OK: allow shyspace in System Settings › General › Login Items";
  });
  await toggle(settings).click();
  await expect(settings.getByText("macOS needs your OK")).toBeVisible();

  await page.evaluate(() => {
    window.fake.openAtLogin.isOn = true;
    window.dispatchEvent(new Event("focus"));
  });

  await expect(toggle(settings)).toBeChecked();
  await expect(settings.getByText("macOS needs your OK")).toHaveCount(0);
});
