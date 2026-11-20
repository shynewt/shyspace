import { expect, type Page, test } from "@playwright/test";
import type { MemberStatus } from "../../src/lib/api";

const HOLIDAY: MemberStatus = { emoji: "🌴", text: "On holiday", until_ms: null, alerts: "nothing" };

function channelLog(page: Page) {
  return page.getByRole("log", { name: "Messages in #general" });
}

async function graceSays(page: Page, text: string, status: MemberStatus | null) {
  await page.evaluate(
    ({ text, status }) => {
      const grace = window.fake.addMember("Grace");
      window.fake.setMemberStatus(grace, status);
      window.fake.receiveMessage(grace, "general", text);
    },
    { text, status },
  );
  await expect(channelLog(page).getByText(text)).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a status emoji shows beside the name once per group, and tells the status on hover", async ({ page }) => {
  await graceSays(page, "Back on Monday", HOLIDAY);
  await page.evaluate(() => window.fake.receiveMessage(window.fake.memberNamed("Grace"), "general", "Ping me later"));
  await expect(channelLog(page).getByText("Ping me later")).toBeVisible();

  const emoji = channelLog(page).getByLabel("Status: 🌴 On holiday", { exact: true });
  await expect(emoji).toHaveCount(1);
  await emoji.hover();
  await expect(page.getByRole("tooltip")).toHaveText("🌴 On holiday");
});

test("a status with an end shows when it ends, also on keyboard focus", async ({ page }) => {
  const inAWeek = Date.now() + 7 * 24 * 60 * 60 * 1000;
  await graceSays(page, "Out for a bit", { ...HOLIDAY, until_ms: inAWeek });

  await channelLog(page)
    .getByLabel(/^Status: 🌴 On holiday · until /)
    .focus();
  await expect(page.getByRole("tooltip")).toContainText(/🌴 On holiday · until \w+ /);
});

test("an emoji-only status shows beside the name", async ({ page }) => {
  await graceSays(page, "Heads down", { ...HOLIDAY, text: "" });

  await expect(channelLog(page).getByLabel("Status: 🌴", { exact: true })).toBeVisible();
});

test("a status without an emoji shows nothing beside the name", async ({ page }) => {
  await graceSays(page, "Around later", { ...HOLIDAY, emoji: "" });

  await expect(channelLog(page).getByRole("article")).toHaveCount(1);
  await expect(channelLog(page).getByLabel(/^Status:/)).toHaveCount(0);
});

test("a status disappears when it ends, without reloading", async ({ page }) => {
  await graceSays(page, "Lunch", { ...HOLIDAY, until_ms: Date.now() + 1500 });
  const emoji = channelLog(page).getByLabel(/^Status: 🌴/);
  await expect(emoji).toBeVisible();

  await expect(emoji).toHaveCount(0);
});

test("a status ends on time even hours after the app started", async ({ page }) => {
  await page.clock.install();
  await page.reload();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.clock.fastForward("03:00:00");
  await graceSays(page, "Coffee", { ...HOLIDAY, until_ms: (await page.evaluate(() => Date.now())) + 1500 });
  const emoji = channelLog(page).getByLabel(/^Status: 🌴/);
  await expect(emoji).toBeVisible();

  await expect(emoji).toHaveCount(0);
});

test("an agent shows no status beside its name", async ({ page }) => {
  await page.evaluate((status) => {
    const agent = window.fake.addMember("Claude", { kind: "bot" });
    window.fake.setMemberStatus(agent, status);
    window.fake.receiveMessage(agent, "general", "Summary ready");
  }, HOLIDAY);
  await expect(channelLog(page).getByText("Summary ready")).toBeVisible();

  await expect(channelLog(page).getByText("Agent", { exact: true })).toBeVisible();
  await expect(channelLog(page).getByLabel(/^Status:/)).toHaveCount(0);
});

test("the thread shows the status beside names too", async ({ page }) => {
  await graceSays(page, "Release notes?", HOLIDAY);
  await channelLog(page).getByRole("article").filter({ hasText: "Release notes?" }).hover();
  await page.getByRole("button", { name: "Reply in thread" }).click();

  const thread = page.getByRole("complementary", { name: "Thread" });
  await expect(thread.getByLabel("Status: 🌴 On holiday", { exact: true })).toBeVisible();
});

test("the mention suggestions show the status emoji", async ({ page }) => {
  await graceSays(page, "Hi", HOLIDAY);
  await page.getByLabel("Message #general").fill("@Gr");

  const option = page.getByRole("listbox", { name: "People to mention" }).getByRole("option", { name: /Grace/ });
  await expect(option.getByLabel("Status: 🌴 On holiday", { exact: true })).toBeVisible();
});

test("the member list shows the emoji beside the name and the text below", async ({ page }) => {
  await graceSays(page, "Hi", HOLIDAY);
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Members" }).first().click();

  const row = settings.getByRole("list", { name: "Members" }).getByRole("listitem").filter({ hasText: "Grace" });
  await expect(row.getByLabel("Status: 🌴 On holiday", { exact: true })).toBeVisible();
  await expect(row).toContainText("On holiday");
  await expect(row).toContainText("· Busy");
});

test("an agent in the member list has its tag but no presence or status", async ({ page }) => {
  await page.evaluate((status) => {
    const agent = window.fake.addMember("Claude", { kind: "bot" });
    window.fake.setMemberStatus(agent, status);
    window.fake.setPresence(agent, "online");
  }, HOLIDAY);
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Members" }).first().click();

  const row = settings.getByRole("list", { name: "Members" }).getByRole("listitem").filter({ hasText: "Claude" });
  await expect(row).toContainText("Agent");
  await expect(row).not.toContainText("Online");
  await expect(row).not.toContainText("On holiday");
});
