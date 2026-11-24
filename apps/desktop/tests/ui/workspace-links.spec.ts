/** Links name their workspace: they open there, and say so plainly when this device isn't in it. */
import { expect, type Page, test } from "@playwright/test";
import { appLink } from "../../src/lib/appLinks";
import { plainPreview } from "../../src/lib/plainPreview";
import { allowClipboard } from "./support";

const ACME = "61636d6500000000";
const ELSEWHERE = "fedcba9876543210";

test.beforeEach(async ({ page, context, browserName }) => {
  await allowClipboard(context, browserName);
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
});

function message(page: Page, text: string) {
  return page.getByRole("article").filter({ hasText: text });
}

async function receive(page: Page, text: string): Promise<{ grace: string; id: string }> {
  return page.evaluate((body) => {
    const grace = window.fake.addMember("Grace");
    return { grace, id: window.fake.receiveMessage(grace, "general", body) };
  }, text);
}

test("Copy link names this workspace", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("The plan is to ship on Friday");
  await composer.press("Enter");
  const before = await page.evaluate(() => navigator.clipboard.readText());
  await message(page, "ship on Friday").hover();
  await page.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Copy link" }).click();

  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).not.toBe(before);
  const link = await page.evaluate(() => navigator.clipboard.readText());
  expect(link).toMatch(new RegExp(`^shy://${ACME}/message/[0-9a-f]+$`));
});

test("a link naming this workspace works like an older one", async ({ page }) => {
  const { id } = await receive(page, "The plan is to ship on Friday");
  const pageId = await page.evaluate(() => window.fake.seedPage("Launch plan", "Steps"));
  const me = await page.evaluate(() => window.fake.session?.me ?? "");
  await receive(
    page,
    `Ask [@Ada](shy://${ACME}/member/${me}), read [it](shy://${ACME}/page/${pageId}) and shy://${ACME}/message/${id}`,
  );

  const linking = message(page, "Ask");
  await expect(linking.locator(".mention-me")).toHaveText("@Ada");
  await expect(linking.locator(".page-chip", { hasText: "Launch plan" })).toBeVisible();
  await expect(linking.getByRole("button", { name: "Message from Grace in #general" })).toContainText("ship on Friday");
});

test("a link to another workspace on this device opens there", async ({ page }) => {
  const { id } = await receive(page, "The plan is to ship on Friday");
  const team = await page.evaluate(() => window.fake.workspaces.add("Team"));
  await receive(page, `Over there: shy://${team}/message/${id}`);

  const chip = message(page, "Over there").getByRole("button", { name: "Open the message in Team" });
  await expect(chip).toContainText("Message in");
  await expect(chip.locator("span.truncate")).toHaveText("Team");
  await chip.click();

  const rail = page.getByRole("navigation", { name: "Workspaces" });
  await expect(rail.getByRole("button", { name: /^Team/ })).toHaveAttribute("aria-current", "page");
  await expect(page.locator(`#message-${id}`)).toBeVisible();
});

test("a link to a workspace this device isn't in says so, with nothing to click", async ({ page }) => {
  await receive(page, `See shy://${ELSEWHERE}/page/0123abcd for the plan`);

  const linking = message(page, "for the plan");
  const text = linking.locator(".prose-chat");
  await expect(text).toContainText("link to a workspace you're not in");
  await expect(text.getByRole("button")).toHaveCount(0);
});

test("a mention of someone in another workspace is plain text, never a chip", async ({ page }) => {
  const me = await page.evaluate(() => window.fake.session?.me ?? "");
  await receive(page, `Thanks [@Ada](shy://${ELSEWHERE}/member/${me}) again`);

  const linking = message(page, "again");
  const text = linking.locator(".prose-chat");
  await expect(text).toContainText("Thanks @Ada again");
  await expect(text.locator(".mention")).toHaveCount(0);
  await expect(text.getByRole("button")).toHaveCount(0);
});

test("previews name this workspace's links and never label another's", () => {
  const titles = (id: string) => (id === "abc" ? "Launch plan" : undefined);
  const names = (id: string) => (id === "e1" ? "Eve" : undefined);
  expect(
    plainPreview(`ask [@Ada](shy://${ACME}/member/e1) about [x](shy://${ACME}/page/abc)`, titles, names, ACME),
  ).toBe("ask @Eve about Launch plan");
  expect(
    plainPreview(`ask [@Ada](shy://${ELSEWHERE}/member/e1) or shy://${ELSEWHERE}/page/abc`, titles, names, ACME),
  ).toBe("ask a link to another workspace or a link to another workspace");
});

test("a workspace with an odd id gets links that still work where they're read", () => {
  expect(appLink("w-old", "page", "abc")).toBe("shy://page/abc");
  expect(appLink(ACME, "page", "abc")).toBe(`shy://${ACME}/page/abc`);
});
