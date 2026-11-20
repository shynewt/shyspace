import { expect, type Page, test } from "@playwright/test";

const card = (page: Page, name: string) => page.getByRole("dialog", { name: `${name}'s profile` });
const chip = (page: Page, name: string) =>
  page.locator(".cm-content").getByRole("button", { name: `${name}, open profile` });

/** A page saying "Ask @Grace now" (and anything after), open in the editor. */
async function openPageMentioning(page: Page, after = "") {
  await page.goto("");
  await page.evaluate((rest) => {
    const grace = window.fake.addMember("Grace");
    window.fake.seedPage("Plans", `Ask [@Grace](shy://member/${grace}) now${rest}`);
  }, after);
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Plans", exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue("Plans");
  await expect(chip(page, "Grace")).toBeVisible();
}

test("clicking a mention in a page opens their card, and Esc goes back to where you were typing", async ({ page }) => {
  await openPageMentioning(page);
  await page.locator(".cm-content").click();
  await page.keyboard.press("End");

  await chip(page, "Grace").click();
  await expect(card(page, "Grace")).toBeVisible();
  await expect(card(page, "Grace")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(card(page, "Grace")).toHaveCount(0);

  await page.keyboard.type("!");
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Plans"))).toMatch(/\) now!$/);
});

test("Ctrl+Enter beside a mention opens their card", async ({ page }) => {
  await openPageMentioning(page);
  await page.locator(".cm-content").click();
  await page.keyboard.press("Home");
  for (let step = 0; step < 4; step += 1) await page.keyboard.press("ArrowRight");

  await page.keyboard.press("Control+Enter");
  await expect(card(page, "Grace").getByRole("button", { name: "Message" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(card(page, "Grace")).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Plans"))).toMatch(/^Ask \[@Grace\]/);
});

test("a card from a page can message them", async ({ page }) => {
  await openPageMentioning(page);
  await chip(page, "Grace").click();
  await card(page, "Grace").getByRole("button", { name: "Message" }).click();

  await expect(page.getByLabel("Message Grace")).toBeVisible();
});

test("a mention of someone unknown in a page opens nothing", async ({ page }) => {
  await openPageMentioning(page, ` and [@Bob](shy://member/${"b".repeat(64)})`);
  await page.locator(".cm-content").getByText("@Bob").click();
  await page.waitForTimeout(300);

  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("pausing on a mention in a page opens nothing", async ({ page }) => {
  await openPageMentioning(page);
  await chip(page, "Grace").hover();
  await page.waitForTimeout(1000);

  await expect(card(page, "Grace")).toHaveCount(0);
});
