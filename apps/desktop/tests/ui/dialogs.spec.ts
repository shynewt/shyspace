import { expect, type Locator, type Page, test } from "@playwright/test";

// Measured without the opening animation, which scales the dialog.
test.use({ reducedMotion: "reduce" });

/** The dialog's border plus its padding. */
const EDGE = 21;
const BELOW_HEADER = 16;

async function box(locator: Locator) {
  const bounds = await locator.boundingBox();
  if (!bounds) throw new Error("not on screen");
  return bounds;
}

/** Content starts the same distance under the header, and the last button sits in the corner. */
async function expectStandardSpacing(dialog: Locator, header: Locator, firstContent: Locator, lastButton: Locator) {
  const [outer, top, first, button] = await Promise.all([box(dialog), box(header), box(firstContent), box(lastButton)]);
  expect(Math.round(first.y - (top.y + top.height))).toBe(BELOW_HEADER);
  expect(Math.round(outer.x + outer.width - (button.x + button.width))).toBe(EDGE);
  expect(Math.round(outer.y + outer.height - (button.y + button.height))).toBe(EDGE);
}

async function openApp(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
}

test("a form dialog has the standard spacing", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: /^Ada\b/ }).click();
  await page.getByRole("menuitem", { name: /^(Set|Edit) status$/ }).click();
  const dialog = page.getByRole("dialog", { name: "Set a status" });

  await expectStandardSpacing(
    dialog,
    dialog.getByRole("heading", { name: "Set a status" }),
    dialog.getByText("Emoji", { exact: true }),
    dialog.getByRole("button", { name: "Set status" }),
  );
});

test("a confirmation has the standard spacing", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Inbox", exact: true }).click();
  await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "general", "news"));
  await page.getByRole("button", { name: "Mark all as read" }).click();
  const dialog = page.getByRole("dialog", { name: "Mark everything as read?" });

  const description = dialog.locator("p").first();
  const lastButton = dialog.getByRole("button", { name: "Mark all as read" });
  await expectStandardSpacing(dialog, description, dialog.getByRole("button", { name: "Cancel" }), lastButton);
});

test("a list dialog has the standard spacing", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("Control+/");
  const dialog = page.getByRole("dialog", { name: "Keyboard shortcuts" });

  const [title, firstShortcut] = [dialog.getByRole("heading"), dialog.getByRole("listitem").first()];
  const [top, first] = await Promise.all([box(title), box(firstShortcut)]);
  expect(Math.round(first.y - (top.y + top.height))).toBe(BELOW_HEADER);
});

test("the shortcuts list renders without warnings, even for a repeated key", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await openApp(page);
  await page.keyboard.press("Control+/");
  const dialog = page.getByRole("dialog", { name: "Keyboard shortcuts" });
  await expect(dialog.getByRole("listitem").filter({ hasText: "Link to another page" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("closing a dialog puts focus back on the button that opened it", async ({ page }) => {
  await page.goto("");
  const opener = page.getByRole("button", { name: "Message someone privately" });
  await opener.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(opener).toBeFocused();
});
