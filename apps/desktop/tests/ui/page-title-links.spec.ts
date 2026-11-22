import { expect, test } from "@playwright/test";

/**
 * Page titles come from other members, so a title must only ever show as
 * text in the chips that link its page, never be read as markdown.
 */
test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
});

/** Mallory links a page with this title; `after` is the rest of the message. */
async function linkPageTitled(page: import("@playwright/test").Page, title: string, after = "today") {
  await page.evaluate(
    ([title, after]) => {
      const id = window.fake.seedPage(title, "");
      window.fake.receiveMessage(window.fake.addMember("Mallory"), "general", `See shy://page/${id} ${after}`);
    },
    [title, after],
  );
  return page.getByRole("article").filter({ hasText: "today" });
}

test("a title that looks like a web address stays the page's name, not a link out", async ({ page }) => {
  const message = await linkPageTitled(page, "<https://evil.example>");
  await expect(message.getByRole("button", { name: "<https://evil.example>", exact: true })).toBeVisible();
  await expect(message.getByRole("link")).toHaveCount(0);
});

test("a title ending in a backslash still makes a page chip", async ({ page }) => {
  const message = await linkPageTitled(page, "Plans\\");
  await expect(message.getByRole("button", { name: "Plans\\", exact: true })).toBeVisible();
  await expect(message).toContainText("See Plans\\ today");
});

test("a backtick in a title doesn't swallow the link or the words after it", async ({ page }) => {
  const message = await linkPageTitled(page, "Notes `", "and `npm test` today");
  await expect(message.getByRole("button", { name: "Notes `", exact: true })).toBeVisible();
  await expect(message.locator("code")).toHaveText("npm test");
});

/** Mallory sends this text, with `{page}` replaced by a page titled "Handbook". */
async function sendWithPage(page: import("@playwright/test").Page, text: string) {
  await page.evaluate((text) => {
    const id = window.fake.seedPage("Handbook", "");
    window.fake.receiveMessage(window.fake.addMember("Mallory"), "general", text.replaceAll("{page}", id));
  }, text);
  return page.getByRole("article").filter({ hasText: "today" });
}

test("a page link keeps its author's label; an empty or blank one shows the page's title", async ({ page }) => {
  const message = await sendWithPage(
    page,
    "[Read this](shy://page/{page}), [](shy://page/{page}) and [ ](shy://page/{page}) today",
  );
  await expect(message.locator(".page-chip")).toHaveText(["Read this", "Handbook", "Handbook"]);
});

test("a chip's label is only text, so it can't hide a link out", async ({ page }) => {
  const message = await sendWithPage(
    page,
    "[<https://evil.example/handbook>](shy://page/{page}) and [@Ada <https://evil.example>](shy://member/ff00) today",
  );
  await expect(message.getByRole("button", { name: "https://evil.example/handbook", exact: true })).toBeVisible();
  await expect(message.getByRole("link")).toHaveCount(0);
});
