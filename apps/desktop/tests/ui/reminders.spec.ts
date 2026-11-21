import { expect, type Locator, type Page, test } from "@playwright/test";
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

function options(picker: Locator) {
  return picker.getByRole("option");
}

test("a delay reads the same with or without “in”", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);

  for (const typed of ["in 5 min", "5min", "5 minutes", "5m"]) {
    await picker.getByLabel("When").fill("");
    await expect(options(picker)).toHaveText(["Pick a date and time…"]);
    await picker.getByLabel("When").fill(typed);
    await expect(options(picker)).toHaveText([/^Today at 10:05 AM\s*in 5 minutes$/, "Pick a date and time…"]);
  }
});

test("a bare number offers what it could mean, and arrows and Enter choose one", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);

  await picker.getByLabel("When").fill("5");

  await expect(options(picker)).toHaveText([
    /^Today at 10:05 AM/,
    /^Today at 3:00 PM/,
    /^Today at 5:00 PM/,
    /^Saturday, Nov 28 at 10:00 AM/,
    "Pick a date and time…",
  ]);
  await page.keyboard.press("ArrowDown");
  await expect(options(picker).nth(1)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Enter");
  await expect(picker).toBeHidden();
  await later(page).click();
  await expect(page.getByRole("list", { name: "Saved for later" })).toContainText("Today 3:00 PM");
});

test("an hour without am or pm on a day offers daytime first", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);

  await picker.getByLabel("When").fill("fri 3");

  await expect(options(picker)).toHaveText([
    /^Friday, Nov 27 at 3:00 PM\s*in 4 days$/,
    /^Friday, Nov 27 at 3:00 AM/,
    "Pick a date and time…",
  ]);
});

test("a date by its month is set with a click", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);

  await picker.getByLabel("When").fill("30 nov 2pm");
  await options(picker).first().click();

  await expect(picker).toBeHidden();
  await later(page).click();
  await expect(page.getByRole("list", { name: "Saved for later" })).toContainText("Nov 30");
});

test("a time that has passed, or isn't a time, says so", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);
  await expect(picker.getByText("Type when, like “in 5 min”, “tomorrow 9” or “fri 3pm”.")).toBeVisible();

  await picker.getByLabel("When").fill("today 8am");
  await expect(picker.getByText("That time has already passed.")).toBeVisible();
  await expect(options(picker)).toHaveText(["Pick a date and time…"]);

  await picker.getByLabel("When").fill("tomorrow nonsense");
  await expect(picker.getByText("No time matches that. Try “in 5 min”, “tomorrow 9” or “fri 3pm”.")).toBeVisible();
});

test("the calendar opens on request and moves by day and week with the arrow keys", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);
  await expect(picker.getByRole("table")).toHaveCount(0);

  await options(picker).filter({ hasText: "Pick a date and time…" }).click();
  const today = picker.getByRole("button", { name: "Monday, November 23", exact: true });
  await expect(today).toHaveAttribute("aria-pressed", "true");
  await expect(picker.getByRole("button", { name: "Sunday, November 22", exact: true })).toBeDisabled();
  await picker.getByLabel("Time").fill("8:00");
  await expect(picker.getByText("That time has already passed.")).toBeVisible();
  await expect(picker.getByRole("button", { name: "Set reminder" })).toBeDisabled();

  await picker.getByLabel("Time").fill("11:00");
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
  await expect(picker.getByText("Set for Tomorrow at 9:00 AM")).toBeVisible();
  await picker.getByLabel("When").fill("tomorrow 2:30pm");
  await page.keyboard.press("Enter");
  await expect(row).toContainText("Tomorrow 2:30 PM");

  await row.click({ button: "right" });
  await chooseByKeyboard(page, "Change reminder", "Don", "Don't remind me");
  await expect(row).toContainText("Read the spec");
  await expect(row).not.toContainText("Tomorrow");
});

test("arrows held with Super or Ctrl leave the calendar's day alone", async ({ page }) => {
  await openApp(page, MONDAY_MORNING);
  const picker = await openPicker(page);
  const today = picker.getByRole("button", { name: "Monday, November 23", exact: true });
  await today.focus();

  for (const key of ["Meta+ArrowRight", "Control+ArrowDown"]) await page.keyboard.press(key);

  await expect(today).toBeFocused();
  await expect(today).toHaveAttribute("aria-pressed", "true");
});
