import { expect, type Page, test } from "@playwright/test";
import { chooseInSubmenu } from "./support";

const MONDAY_MORNING = new Date("2026-11-23T10:00:00");
const FRIDAY_EVENING = new Date("2026-11-27T19:00:00");

async function openApp(page: Page, at: Date) {
  await page.clock.setFixedTime(at);
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "general", "Read the spec"));
}

async function openRemindMe(page: Page) {
  await page.getByRole("article").filter({ hasText: "Read the spec" }).hover();
  await page.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Remind me" }).hover();
}

/** Picks from an open menu's submenu by keyboard, as its last items are far from the trigger. */
async function chooseByKeyboard(page: Page, submenu: string, typed: string, choice: string) {
  await page.getByRole("menuitem", { name: submenu }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("menuitemradio").first()).toBeFocused();
  await page.keyboard.type(typed);
  await expect(page.getByRole("menuitemradio", { name: choice })).toBeFocused();
  await page.keyboard.press("Enter");
}

async function chooseCustom(page: Page, submenu: string) {
  await chooseByKeyboard(page, submenu, "Cu", "Custom…");
  return page.getByRole("dialog", { name: "Remind me" });
}

async function openPicker(page: Page) {
  await openRemindMe(page);
  return chooseCustom(page, "Remind me");
}

function later(page: Page) {
  return page.getByRole("button", { name: "Later", exact: true });
}

test("the presets fit the moment: later today and this weekend on a weekday morning", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);

  await openRemindMe(page);

  await expect(page.getByRole("menuitemradio")).toHaveText([
    "In 20 minutes",
    "In 1 hour",
    "Later today · 1:00 PM",
    "Tomorrow · 9:00 AM",
    "This weekend · Sat 9:00 AM",
    "Next week · Mon 9:00 AM",
    "Custom…",
  ]);
});

test("on a Friday evening there's no later today and no this weekend", async ({ page }) => {
  await openApp(page, FRIDAY_EVENING);

  await openRemindMe(page);

  await expect(page.getByRole("menuitemradio")).toHaveText([
    "In 20 minutes",
    "In 1 hour",
    "Tomorrow · 9:00 AM",
    "Next week · Mon 9:00 AM",
    "Custom…",
  ]);
});

test("a typed time is shown in words, then set", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);

  await picker.getByLabel("When").fill("fri 3pm");

  await expect(picker.getByText("Friday, Nov 27 at 3:00 PM")).toBeVisible();
  await picker.getByRole("button", { name: "Set reminder" }).click();
  await expect(picker).toBeHidden();
  await later(page).click();
  await expect(page.getByRole("list", { name: "Saved for later" })).toContainText("Nov 27");
});

test("a time that has passed can't be set", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);

  await picker.getByLabel("Time").fill("8:00");

  await expect(picker.getByText("That time has already passed.")).toBeVisible();
  await expect(picker.getByRole("button", { name: "Set reminder" })).toBeDisabled();
  await picker.getByLabel("When").fill("tomorrow nonsense");
  await expect(picker.getByText("Try “tomorrow 9”, “fri 3pm” or “in 2h”.")).toBeVisible();
});

test("the calendar moves by day and week with the arrow keys", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);
  const today = picker.getByRole("button", { name: "Monday, November 23", exact: true });
  await expect(today).toHaveAttribute("aria-pressed", "true");
  await expect(picker.getByRole("button", { name: "Sunday, November 22", exact: true })).toBeDisabled();

  await today.focus();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");

  await expect(picker.getByRole("button", { name: "Tuesday, December 1", exact: true })).toBeFocused();
  await expect(picker.getByText("December 2026")).toBeVisible();
  await expect(picker.getByText("Tuesday, Dec 1 at 11:00 AM")).toBeVisible();
});

test("a reminder can be changed or dropped from Later", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  await openRemindMe(page);
  await chooseInSubmenu(page, "Remind me", "Tomorrow · 9:00 AM");
  await later(page).click();
  const row = page.getByRole("list", { name: "Saved for later" }).getByRole("listitem");
  await expect(row).toContainText("Tomorrow 9:00 AM");

  await row.click({ button: "right" });
  const picker = await chooseCustom(page, "Change reminder");
  await expect(picker.getByText("Tuesday, Nov 24 at 9:00 AM")).toBeVisible();
  await picker.getByLabel("Time").fill("14:30");
  await picker.getByRole("button", { name: "Set reminder" }).click();
  await expect(row).toContainText("Tomorrow 2:30 PM");

  await row.click({ button: "right" });
  await chooseByKeyboard(page, "Change reminder", "Don", "Don't remind me");
  await expect(row).toContainText("Read the spec");
  await expect(row).not.toContainText("Tomorrow");
});
