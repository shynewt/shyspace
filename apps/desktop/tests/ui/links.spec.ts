import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
});

async function copyFromMessage(page: import("@playwright/test").Page, text: string, action: string) {
  await page.getByRole("article").filter({ hasText: text }).first().hover();
  await page.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: action }).click();
  return page.evaluate(() => navigator.clipboard.readText());
}

test("copy text gives the message's raw markdown", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Use **bold** and `code`");
  await composer.press("Enter");

  expect(await copyFromMessage(page, "Use bold", "Copy text")).toBe("Use **bold** and `code`");
});

test("a pasted message link shows a card that jumps to the message", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("The plan is to ship on Friday");
  await composer.press("Enter");
  const link = await copyFromMessage(page, "ship on Friday", "Copy link");
  expect(link).toMatch(/^shy:\/\/message\/[0-9a-f]+$/);

  await composer.fill(`See ${link}`);
  await composer.press("Enter");
  const card = page.getByRole("button", { name: "Message from Ada in #general" });
  await expect(card).toContainText("ship on Friday");
  await card.click();
  await expect(page.locator(`#message-${link.split("/").pop()}`)).toBeVisible();
});
