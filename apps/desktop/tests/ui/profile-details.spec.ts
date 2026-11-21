import { expect, type Page, test } from "@playwright/test";
import type { ProfileDetails } from "../../src/lib/api";
import { allowClipboard, openedLinks, recordOpenedLinks } from "./support";

test.use({ locale: "en-US", timezoneId: "Europe/Lisbon" });

/** Monday 23 November 2026, 23:42 in Lisbon (UTC+0 in winter). */
const MONDAY_LATE = Date.UTC(2026, 10, 23, 23, 42);
const WEEKDAYS_NINE_TO_FIVE = { days: 0b001_1111, start_minute: 9 * 60, end_minute: 17 * 60 };

function channelLog(page: Page) {
  return page.getByRole("log", { name: "Messages in #general" });
}

function card(page: Page, name: string) {
  return page.getByRole("dialog", { name: `${name}'s profile` });
}

async function openApp(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
}

/** Grace, with `details`, has written in #general; her card is opened by a click on her name. */
async function openGracesCard(page: Page, details: Partial<ProfileDetails>) {
  await page.evaluate((details) => {
    const grace = window.fake.addMember("Grace");
    window.fake.setMemberDetails(grace, details);
    window.fake.receiveMessage(grace, "general", "Morning all");
  }, details);
  await channelLog(page).getByRole("button", { name: "Grace", exact: true }).click();
  await expect(card(page, "Grace")).toBeVisible();
  return card(page, "Grace");
}

/** The colour a token gives text, as the browser computes it. */
function tokenColor(page: Page, token: string) {
  return page.evaluate((token) => {
    const probe = document.createElement("span");
    probe.style.color = `var(${token})`;
    document.body.append(probe);
    const color = getComputedStyle(probe).color;
    probe.remove();
    return color;
  }, token);
}

test("a card shows only what they filled in", async ({ page }) => {
  await openApp(page);
  const grace = await openGracesCard(page, { title: "Engineer", pronouns: "she/her", location: "Berlin office" });
  await expect(grace.getByRole("heading", { name: "Grace" })).toBeVisible();
  await expect(grace).toContainText("she/her");
  await expect(grace).toContainText("Engineer");
  const details = grace.getByRole("list", { name: "Details" });
  await expect(details.getByRole("listitem")).toHaveCount(1);
  await expect(details).toContainText("Berlin office");
  await expect(grace.getByRole("link")).toHaveCount(0);
  await expect(grace).not.toContainText("local time");
});

test("a card without details has no detail rows", async ({ page }) => {
  await openApp(page);
  const grace = await openGracesCard(page, {});
  await expect(grace.getByRole("list", { name: "Details" })).toHaveCount(0);
});

test("their local time says how far ahead they are and when they're outside working hours", async ({ page }) => {
  await page.clock.setFixedTime(MONDAY_LATE);
  await openApp(page);
  const grace = await openGracesCard(page, { time_zone: "Asia/Tokyo", working_hours: WEEKDAYS_NINE_TO_FIVE });
  const row = grace.getByRole("listitem").filter({ hasText: "local time" });
  await expect(row).toHaveText("8:42 AM local time · 9 h ahead" + "Outside working hours");
  const warning = row.getByText("Outside working hours", { exact: true });
  await expect(warning).toHaveCSS("color", await tokenColor(page, "--warning-text"));
  expect(await warning.evaluate((line) => line.scrollWidth <= line.clientWidth)).toBe(true);
});

test("within their hours, the local time carries no warning", async ({ page }) => {
  await page.clock.setFixedTime(Date.UTC(2026, 10, 23, 1, 30));
  await openApp(page);
  const grace = await openGracesCard(page, { time_zone: "Asia/Tokyo", working_hours: WEEKDAYS_NINE_TO_FIVE });
  await expect(grace.getByRole("listitem").filter({ hasText: "local time" })).toHaveText(
    "10:30 AM local time · 9 h ahead",
  );
});

test("hours that cross midnight count on the night they start", async ({ page }) => {
  const weekendNights = { days: 0b111_0000, start_minute: 22 * 60, end_minute: 6 * 60 };
  // Saturday 03:00 in Lisbon: still Friday's shift.
  await page.clock.setFixedTime(Date.UTC(2026, 10, 28, 3, 0));
  await openApp(page);
  const grace = await openGracesCard(page, { time_zone: "Europe/Lisbon", working_hours: weekendNights });
  await expect(grace.getByRole("listitem").filter({ hasText: "local time" })).toHaveText(
    "3:00 AM local time · Same time as you",
  );
});

test("a night outside the shifts is outside working hours", async ({ page }) => {
  const weekendNights = { days: 0b111_0000, start_minute: 22 * 60, end_minute: 6 * 60 };
  // Thursday 03:00 in Lisbon: Wednesday had no shift.
  await page.clock.setFixedTime(Date.UTC(2026, 10, 26, 3, 0));
  await openApp(page);
  const grace = await openGracesCard(page, { time_zone: "Europe/Lisbon", working_hours: weekendNights });
  await expect(grace.getByRole("listitem").filter({ hasText: "local time" })).toHaveText(
    "3:00 AM local time · Same time as you" + "Outside working hours",
  );
});

test("the local time keeps up with the clock", async ({ page }) => {
  await page.clock.install({ time: Date.UTC(2026, 10, 23, 10, 0, 30) });
  await openApp(page);
  const grace = await openGracesCard(page, { time_zone: "Europe/Lisbon" });
  const row = grace.getByRole("listitem").filter({ hasText: "local time" });
  await expect(row).toHaveText("10:00 AM local time · Same time as you");
  await page.clock.runFor(60_000);
  await expect(row).toHaveText("10:01 AM local time · Same time as you");
});

test("contact rows open with the system and copy with a check that fades back", async ({
  page,
  context,
  browserName,
}) => {
  await allowClipboard(context, browserName);
  await openApp(page);
  await recordOpenedLinks(page);
  const grace = await openGracesCard(page, {
    email: "grace@example.com",
    phone: "+351 21 123 4567",
    website: "https://grace.example.com/",
  });
  await grace.getByRole("link", { name: "grace@example.com" }).click();
  await grace.getByRole("link", { name: "+351 21 123 4567" }).click();
  await grace.getByRole("link", { name: "grace.example.com" }).click();
  expect(await openedLinks(page)).toEqual([
    "mailto:grace@example.com",
    "tel:+351211234567",
    "https://grace.example.com/",
  ]);

  const copy = grace.getByRole("button", { name: "Copy email address" });
  await copy.click();
  await expect(copy.locator("[data-copied]")).toBeVisible();
  await expect(grace.getByRole("status").filter({ hasText: "Copied" })).toHaveCount(1);
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("grace@example.com");
  await expect(copy.locator("[data-copied]")).toHaveCount(0, { timeout: 3000 });
  await expect(grace.getByRole("status").filter({ hasText: "Copied" })).toHaveCount(0);
});

test("a website that isn't a web address shows nothing", async ({ page }) => {
  await openApp(page);
  const grace = await openGracesCard(page, { website: "javascript:alert(1)", location: "Lisbon" });
  await expect(grace.getByRole("link")).toHaveCount(0);
});

test("a long bio is cut to three lines with a way to read it all", async ({ page }) => {
  await openApp(page);
  const bio = `${"Builds compilers and teaches. ".repeat(9)}See https://grace.example.com.`;
  const grace = await openGracesCard(page, { bio, time_zone: "Europe/Lisbon", working_hours: WEEKDAYS_NINE_TO_FIVE });
  await grace.getByRole("button", { name: "View full profile" }).click();

  const full = page.getByRole("dialog", { name: "Grace's full profile" });
  await expect(full).toBeVisible();
  await expect(card(page, "Grace")).toHaveCount(0);
  await expect(full.getByText(/Builds compilers/)).toHaveText(bio);
  await expect(full.getByRole("link", { name: "https://grace.example.com" })).toBeVisible();
  await expect(full.getByRole("listitem").filter({ hasText: "Mon–Fri, 9:00 AM–5:00 PM" })).toBeVisible();
});

test("a short bio shows whole, with no extra link", async ({ page }) => {
  await openApp(page);
  const grace = await openGracesCard(page, { bio: "Builds compilers." });
  await expect(grace).toContainText("Builds compilers.");
  await expect(grace.getByRole("button", { name: "View full profile" })).toHaveCount(0);
});

test("the keyboard reaches the more menu and the full profile, and comes back", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.setMemberDetails(grace, { title: "Engineer" });
    window.fake.receiveMessage(grace, "general", "Morning all");
  });
  const name = channelLog(page).getByRole("button", { name: "Grace", exact: true });
  await name.focus();
  await page.keyboard.press("Enter");
  const grace = card(page, "Grace");
  await expect(grace.getByRole("button", { name: "Message" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(grace.getByRole("button", { name: "Mention" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(grace.getByRole("button", { name: "More actions" })).toBeFocused();

  await page.keyboard.press("Enter");
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem")).toHaveText([
    "View full profile",
    "Copy name",
    "View their messages",
    "View in members",
  ]);
  await page.keyboard.press("ArrowDown");
  await expect(menu.getByRole("menuitem", { name: "View full profile" })).toBeFocused();
  await page.keyboard.press("Enter");

  const full = page.getByRole("dialog", { name: "Grace's full profile" });
  await expect(full).toBeVisible();
  await expect(full).toContainText("Engineer");
  await page.keyboard.press("Escape");
  await expect(full).toHaveCount(0);
  await expect(name).toBeFocused();
});

test("view their messages searches for what they wrote", async ({ page }) => {
  await openApp(page);
  const grace = await openGracesCard(page, {});
  await grace.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "View their messages" }).click();
  await expect(page.getByLabel("Search messages and pages")).toHaveValue('from:"Grace"');
  await expect(page.getByLabel("Search messages and pages")).toBeFocused();
});

test("your own card offers your status and editing your profile", async ({ page }) => {
  await openApp(page);
  const composer = page.getByLabel("Message #general");
  await composer.fill("Mine");
  await composer.press("Enter");
  await channelLog(page).getByRole("button", { name: "Ada", exact: true }).click();
  const ada = card(page, "Ada");
  await expect(ada.getByRole("button", { name: "Message" })).toHaveCount(0);
  await expect(ada.getByRole("button", { name: "Set status" })).toBeVisible();
  await ada.getByRole("button", { name: "More actions" }).click();
  await expect(page.getByRole("menu").getByRole("menuitem")).toHaveText(["View full profile", "View in members"]);
  await page.keyboard.press("Escape");

  await ada.getByRole("button", { name: "Edit profile" }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByLabel("Title")).toBeFocused();
});

test("set status from your card opens the status dialog", async ({ page }) => {
  await openApp(page);
  const composer = page.getByLabel("Message #general");
  await composer.fill("Mine");
  await composer.press("Enter");
  await channelLog(page).getByRole("button", { name: "Ada", exact: true }).click();
  await card(page, "Ada").getByRole("button", { name: "Set status" }).click();
  await expect(page.getByRole("dialog", { name: "Set a status" })).toBeVisible();
});

test("an agent's card offers its messages, and agent settings to the admin", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() =>
    window.fake.receiveMessage(window.fake.addMember("Claude", { kind: "bot" }), "general", "Summary ready"),
  );
  await channelLog(page).getByRole("button", { name: "Claude", exact: true }).click();
  const claude = card(page, "Claude");
  await expect(claude.getByRole("button", { name: "Mention" })).toBeVisible();
  await claude.getByRole("button", { name: "More actions" }).click();
  await expect(page.getByRole("menuitem", { name: "View its messages" })).toBeVisible();
  await page.getByRole("menuitem", { name: "Agent settings" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" }).getByRole("tab", { name: "Agents" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("using a card that opened on hover keeps it open", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", "Morning all");
  });
  await channelLog(page).getByRole("button", { name: "Grace", exact: true }).hover();
  const grace = card(page, "Grace");
  await expect(grace).toBeVisible();
  await grace.getByRole("button", { name: "More actions" }).click();
  const viewInMembers = page.getByRole("menuitem", { name: "View in members" });
  // Longer than a hover card lingers once the pointer leaves it: the menu would go with the card.
  await viewInMembers.hover();
  await page.waitForTimeout(600);
  await viewInMembers.click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
});

test("the header's name, title and tags sit centred within the avatar's height", async ({ page }) => {
  await openApp(page);
  const graceCard = await openGracesCard(page, { pronouns: "she/her", title: "Rear admiral" });
  const { avatar, text } = await graceCard.getByRole("heading", { name: "Grace" }).evaluate((heading) => {
    const column = heading.parentElement?.parentElement;
    const avatarBox = column?.previousElementSibling?.getBoundingClientRect();
    const textBox = column?.getBoundingClientRect();
    if (!avatarBox || !textBox) throw new Error("header layout not found");
    return {
      avatar: { top: avatarBox.top, bottom: avatarBox.bottom },
      text: { top: textBox.top, bottom: textBox.bottom },
    };
  });
  expect(text.top).toBeGreaterThanOrEqual(avatar.top);
  expect(text.bottom).toBeLessThanOrEqual(avatar.bottom);
  expect(Math.abs((text.top + text.bottom) / 2 - (avatar.top + avatar.bottom) / 2)).toBeLessThanOrEqual(1);
});
