import { expect, type Page, test } from "@playwright/test";

/** A page long enough that its end is out of view when it opens. */
const LONG_PAGE = Array.from({ length: 80 }, (_, line) => `Line ${line + 1} of the plan`).join("\n\n");

const alertsShown = (page: Page) => page.evaluate(() => window.fake.alertsShown);
const mentionLine = (page: Page) => page.locator(".cm-mention-target");

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate((text) => {
    window.fake.addMember("Grace");
    window.fake.seedPage("Plan", text);
  }, LONG_PAGE);
});

test("a mention in a page notifies you, and the notification opens the page at it", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.receivePageMention("Plan", window.fake.memberNamed("Grace"), "Can you check ", " before Friday?"),
  );
  await expect
    .poll(() => alertsShown(page))
    .toEqual([
      { title: "Grace mentioned you in 📄 Plan", body: expect.stringMatching(/^Can you check @.+ before Friday\?$/) },
    ]);

  await page.evaluate(() => window.fake.clickAlert(0));

  await expect(page.getByLabel("Page title")).toHaveValue("Plan");
  await expect(mentionLine(page)).toContainText("before Friday?");
  await expect(mentionLine(page)).toBeInViewport();
  await expect(page.locator(".cm-content")).toBeFocused();
  await expect(page.getByRole("button", { name: "Inbox", exact: true })).toBeVisible();
});

test("with notification text hidden, a page mention shows neither its text nor the page's title", async ({ page }) => {
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Notifications" }).click();
  await settings.getByRole("radio", { name: "Hide text" }).click();
  await page.keyboard.press("Escape");

  await page.evaluate(() => window.fake.receivePageMention("Plan", window.fake.memberNamed("Grace"), "Salary for "));

  await expect.poll(() => alertsShown(page)).toEqual([{ title: "Grace", body: "Mentioned you in a page" }]);
});

test("looking at the page, a new mention there neither notifies nor waits in the inbox", async ({ page }) => {
  await page.evaluate(() => window.fake.seedPage("Short", "Agenda"));
  await page.getByRole("button", { name: "Short", exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue("Short");

  await page.evaluate(() => window.fake.receivePageMention("Short", window.fake.memberNamed("Grace"), "Over to "));

  await expect(page.locator(".cm-content")).toContainText("Over to");
  await expect(page.getByRole("button", { name: "Inbox", exact: true })).toBeVisible();
  expect(await alertsShown(page)).toEqual([]);
});

test("a page mention waits in the inbox and opens the page at it", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.receivePageMention("Plan", window.fake.memberNamed("Grace"), "Ask ", " about keys", false),
  );
  const inboxLink = page.getByRole("button", { name: "Inbox, 1 unread" });
  await expect(inboxLink).toBeVisible();
  await inboxLink.click();

  const entry = page.getByRole("region", { name: "Mentions and replies" }).getByRole("button");
  await expect(entry).toHaveCount(1);
  await expect(entry).toContainText("Grace mentioned you in");
  await expect(entry).toContainText("Plan");
  await expect(entry).toContainText(/Ask @.+ about keys/);
  await entry.click();

  await expect(page.getByLabel("Page title")).toHaveValue("Plan");
  await expect(mentionLine(page)).toContainText("about keys");
  await expect(mentionLine(page)).toBeInViewport();
  await page.getByRole("button", { name: "Inbox", exact: true }).click();
  await expect(page.getByText("You're all caught up")).toBeVisible();
});

test("a mention whose text was deleted still opens the page where it was", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.receivePageMention("Plan", window.fake.memberNamed("Grace"), "Ask ", " about keys", false);
    const text = window.fake.pageText("Plan");
    const start = text.indexOf("\n\nAsk ");
    window.fake.receivePageEdit("Plan", { at: start, remove: text.length - start });
    window.fake.receivePageEdit("Plan", { at: start, insert: "\n\nKeys are sorted" });
  });
  await page.getByRole("button", { name: "Inbox, 1 unread" }).click();
  await page.getByRole("region", { name: "Mentions and replies" }).getByRole("button").click();

  await expect(page.getByLabel("Page title")).toHaveValue("Plan");
  await expect(mentionLine(page)).toHaveCount(1);
  await expect(mentionLine(page)).toBeInViewport();
});

test("typing clears the mark on the line a mention opened at", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.receivePageMention("Plan", window.fake.memberNamed("Grace"), "Ask ", " about keys", false),
  );
  await page.getByRole("button", { name: "Inbox, 1 unread" }).click();
  await page.getByRole("region", { name: "Mentions and replies" }).getByRole("button").click();
  await expect(mentionLine(page)).toHaveCount(1);

  await page.keyboard.type("Sure. ");

  await expect(mentionLine(page)).toHaveCount(0);
});

test("a page offers no @channel or @here and shows existing ones as plain text", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.seedPage("Notes", "Heads up [@channel](shy://mention/channel) and [@here](shy://mention/here)"),
  );
  await page.getByRole("button", { name: "Notes", exact: true }).click();
  const editor = page.locator(".cm-content");
  await expect(editor).toContainText("Heads up @channel and @here");
  await expect(editor.locator(".cm-md-mention")).toHaveCount(0);

  await editor.click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type(" @");
  const people = page.getByRole("listbox", { name: "People to mention" });
  await expect(people.getByRole("option", { name: /Grace/ })).toBeVisible();
  await expect(people.getByRole("option", { name: /channel|here/ })).toHaveCount(0);
});
