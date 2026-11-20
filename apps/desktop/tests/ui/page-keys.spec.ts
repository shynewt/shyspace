import { expect, type Page, test } from "@playwright/test";

const pageText = (page: Page) => page.evaluate(() => window.fake.pageText("Keys"));

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();
  await page.getByLabel("Page title").fill("Keys");
  await page.getByLabel("Page title").press("Enter");
  await expect(page.locator(".cm-content")).toBeFocused();
});

test("bold, italic and code keys wrap, unwrap and step out", async ({ page }) => {
  await page.keyboard.type("plain ");
  await page.keyboard.press("ControlOrMeta+b");
  await page.keyboard.type("bold");
  await page.keyboard.press("ControlOrMeta+b");
  await page.keyboard.type(" and code");
  await page.keyboard.press("Shift+ControlOrMeta+ArrowLeft");
  await page.keyboard.press("ControlOrMeta+e");
  await expect.poll(() => pageText(page)).toBe("plain **bold** and `code`");

  await page.keyboard.press("ControlOrMeta+e");
  await page.keyboard.press("ControlOrMeta+i");
  await expect.poll(() => pageText(page)).toBe("plain **bold** and *code*");
});

test("ctrl+k makes the selection a link; with nothing selected it opens search", async ({ page }) => {
  await page.keyboard.type("the spec");
  await page.keyboard.press("Shift+Home");
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.type("https://example.com");
  await expect.poll(() => pageText(page)).toBe("[the spec](https://example.com)");
  await expect(page.getByLabel("Search messages and pages")).toHaveCount(0);

  await page.keyboard.press("End");
  await page.keyboard.press("ControlOrMeta+k");
  await expect(page.getByLabel("Search messages and pages")).toBeVisible();
});

test("undo and redo work with every usual key", async ({ page }) => {
  await page.keyboard.type("first");
  await expect.poll(() => pageText(page)).toBe("first");

  await page.keyboard.press("ControlOrMeta+z");
  await expect.poll(() => pageText(page)).toBe("");
  // KeyZ, unlike "z", sends the "Z" a real keyboard gives with Shift held.
  await page.keyboard.press("ControlOrMeta+Shift+KeyZ");
  await expect.poll(() => pageText(page)).toBe("first");
  await page.keyboard.press("ControlOrMeta+z");
  await expect.poll(() => pageText(page)).toBe("");
  await page.keyboard.press("Control+y");
  await expect.poll(() => pageText(page)).toBe("first");
});

test("ctrl+enter checks off the item you're on", async ({ page }) => {
  await page.keyboard.type("- [ ] call the bank");
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect.poll(() => pageText(page)).toBe("- [x] call the bank");
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect.poll(() => pageText(page)).toBe("- [ ] call the bank");
});

test("the shortcut cheat sheet opens from the editor without changing the page", async ({ page }) => {
  await page.keyboard.type("text");
  await page.keyboard.press("ControlOrMeta+/");
  await expect(page.getByRole("dialog", { name: "Keyboard shortcuts" })).toContainText("Bold");
  await expect.poll(() => pageText(page)).toBe("text");
});
