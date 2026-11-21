/** Profile details where people already appear: the @ suggestions, the member list and a DM's message box. */
import { expect, type Page, test } from "@playwright/test";
import type { ProfileDetails } from "../../src/lib/api";

test.use({ locale: "en-US", timezoneId: "Europe/Lisbon" });

const WEEKDAYS_NINE_TO_FIVE = { days: 0b001_1111, start_minute: 9 * 60, end_minute: 17 * 60 };
/** Monday 23 November 2026 in Lisbon (UTC+0 in winter). */
const MONDAY_AT = (hour: number, minute: number) => Date.UTC(2026, 10, 23, hour, minute);

async function openApp(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
}

async function addPerson(page: Page, name: string, details: Partial<ProfileDetails>) {
  await page.evaluate(
    ({ name, details }) => {
      const id = window.fake.addMember(name);
      window.fake.setMemberDetails(id, details);
      window.fake.receiveMessage(id, "general", `Hello from ${name}`);
    },
    { name, details },
  );
}

async function openDmWith(page: Page, name: string) {
  await page.getByRole("log", { name: "Messages in #general" }).getByRole("button", { name, exact: true }).click();
  await page
    .getByRole("dialog", { name: `${name}'s profile` })
    .getByRole("button", { name: "Message" })
    .click();
  await expect(page.getByLabel(`Message ${name}`)).toBeVisible();
}

test("the @ suggestions show pronouns after the name", async ({ page }) => {
  await openApp(page);
  await addPerson(page, "Grace", { pronouns: "she/her", title: "Engineer" });
  await page.getByLabel("Message #general").pressSequentially("Thanks @Gr");
  const grace = page
    .getByRole("listbox", { name: "People and pages to mention" })
    .getByRole("option", { name: /Grace/ });
  await expect(grace).toContainText("she/her");
});

test("the member list shows each title under the name", async ({ page }) => {
  await openApp(page);
  await addPerson(page, "Grace", { title: "Engineer" });
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Members" }).first().click();
  await expect(settings.getByRole("listitem").filter({ hasText: "Grace" })).toContainText("Engineer · Not reachable");
});

test("late at night for them, a DM says what time it is there", async ({ page }) => {
  await page.clock.setFixedTime(MONDAY_AT(23, 10));
  await openApp(page);
  await addPerson(page, "Theo", { time_zone: "Europe/Lisbon" });
  await openDmWith(page, "Theo");
  await expect(page.getByText("It's 11:10 PM for Theo")).toBeVisible();
  await expect(page.getByLabel("Message Theo")).toBeEnabled();
});

test("outside their working hours, the DM hint says so", async ({ page }) => {
  await page.clock.setFixedTime(MONDAY_AT(23, 10));
  await openApp(page);
  await addPerson(page, "Theo", { time_zone: "Europe/Lisbon", working_hours: WEEKDAYS_NINE_TO_FIVE });
  await openDmWith(page, "Theo");
  await expect(page.getByText("It's 11:10 PM for Theo · outside their working hours")).toBeVisible();
});

test("during their day, a DM shows no hint", async ({ page }) => {
  await page.clock.setFixedTime(MONDAY_AT(10, 0));
  await openApp(page);
  await addPerson(page, "Theo", { time_zone: "Europe/Lisbon", working_hours: WEEKDAYS_NINE_TO_FIVE });
  await openDmWith(page, "Theo");
  await expect(page.getByText(/for Theo/)).toHaveCount(0);
});

test("the hint goes when their working hours start", async ({ page }) => {
  await page.clock.install({ time: MONDAY_AT(8, 59) });
  await openApp(page);
  await addPerson(page, "Theo", { time_zone: "Europe/Lisbon", working_hours: WEEKDAYS_NINE_TO_FIVE });
  await openDmWith(page, "Theo");
  await expect(page.getByText("It's 8:59 AM for Theo · outside their working hours")).toBeVisible();
  await page.clock.runFor(61_000);
  await expect(page.getByText(/for Theo/)).toHaveCount(0);
});
