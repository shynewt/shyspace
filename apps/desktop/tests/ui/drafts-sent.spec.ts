import { expect, type Page, test } from "@playwright/test";
import { allowClipboard } from "./support";

const DAY_MS = 24 * 60 * 60 * 1000;

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #random")).toBeVisible();
});

function draftsAndSent(page: Page) {
  return page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: /^Drafts & sent/ });
}

async function openSent(page: Page) {
  await draftsAndSent(page).click();
  await page.getByRole("tab", { name: "Sent" }).click();
}

async function send(page: Page, channel: string, text: string) {
  await page.getByRole("button", { name: channel, exact: true }).click();
  const composer = page.getByLabel(`Message #${channel}`);
  await composer.fill(text);
  await composer.press("Enter");
  await expect(page.getByRole("article").filter({ hasText: text })).toBeVisible();
}

test("the sidebar counts drafts quietly, and the count follows them", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Drafts & sent", exact: true })).toBeVisible();

  await page.getByLabel("Message #random").fill("half a thought");
  await page.getByRole("button", { name: "general", exact: true }).click();
  const oneDraft = page.getByRole("button", { name: "Drafts & sent, 1 draft" });
  await expect(oneDraft).toBeVisible();
  await expect(oneDraft).not.toHaveCSS("font-weight", "600");

  await page.getByLabel("Message #general").fill("another one");
  await page.getByRole("button", { name: "random, draft" }).click();
  await expect(page.getByRole("button", { name: "Drafts & sent, 2 drafts" })).toBeVisible();

  await page.getByLabel("Message #random").press("Enter");
  await expect(oneDraft).toBeVisible();
});

test("a draft opens in place, with its text in the message box", async ({ page }) => {
  await page.getByLabel("Message #random").fill("finish this later");
  await draftsAndSent(page).click();

  const row = page.getByRole("list", { name: "Drafts" }).getByRole("button", { name: /finish this later/ });
  await expect(row).toContainText("random");
  await row.click();
  await expect(page.getByLabel("Message #random")).toHaveValue("finish this later");
});

test("a thread draft is labelled and opens its thread", async ({ page }) => {
  await page.evaluate(() => {
    const root = window.fake.receiveMessage(window.fake.addMember("Grace"), "general", "Who reviews?");
    window.fake.handle("set_draft", { target: root, text: "I can, after lunch" });
  });
  await draftsAndSent(page).click();

  const row = page.getByRole("list", { name: "Drafts" }).getByRole("button", { name: /I can, after lunch/ });
  await expect(row).toContainText("Thread in");
  await expect(row).toContainText("general");
  await row.click();
  const thread = page.getByRole("complementary", { name: "Thread" });
  await expect(thread.getByLabel("Reply in thread")).toHaveValue("I can, after lunch");
});

test("a discarded draft comes back with Undo", async ({ page }) => {
  await page.getByLabel("Message #random").fill("maybe not");
  await draftsAndSent(page).click();
  const row = page.getByRole("list", { name: "Drafts" }).getByRole("listitem").filter({ hasText: "maybe not" });

  await row.hover();
  await row.getByRole("button", { name: "Discard draft" }).click();
  await expect(page.getByText("No drafts")).toBeVisible();
  await expect(page.getByRole("button", { name: "Drafts & sent", exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Undo" }).click();
  await expect(page.getByRole("list", { name: "Drafts" })).toContainText("maybe not");
  await expect(page.getByRole("button", { name: "Drafts & sent, 1 draft" })).toBeVisible();
});

test("a draft untouched for more than a week is marked old", async ({ page }) => {
  const now = Date.now();
  // An open message box keeps its own draft, so the drafts are written from elsewhere.
  await draftsAndSent(page).click();
  await page.clock.setFixedTime(now - 8 * DAY_MS);
  await page.evaluate(() =>
    window.fake.handle("set_draft", { target: window.fake.channelId("random"), text: "stale" }),
  );
  await page.clock.setFixedTime(now - 6 * DAY_MS);
  await page.evaluate(() =>
    window.fake.handle("set_draft", { target: window.fake.channelId("general"), text: "fresh" }),
  );
  await page.clock.setFixedTime(now);
  await page.getByRole("button", { name: "Later", exact: true }).click();
  await draftsAndSent(page).click();

  const drafts = page.getByRole("list", { name: "Drafts" });
  await expect(drafts.getByRole("button", { name: /stale/ })).toContainText("Old");
  await expect(drafts.getByRole("button", { name: /fresh/ })).not.toContainText("Old");
});

test("with nothing written yet, each tab says so in one line", async ({ page }) => {
  await draftsAndSent(page).click();
  await expect(page.getByText("No drafts")).toBeVisible();
  await expect(page.getByText("Anything you start typing and leave is kept here.")).toBeVisible();

  await page.getByRole("tab", { name: "Sent" }).click();
  await expect(page.getByText("Nothing sent yet")).toBeVisible();
});

test("sent groups your messages by day and shows how people responded", async ({ page }) => {
  const now = Date.now();
  await page.clock.setFixedTime(now - DAY_MS);
  await send(page, "general", "Shipping notes are up");
  await page.clock.setFixedTime(now);
  await send(page, "general", "Who reviews?");
  await send(page, "random", "Can someone check the build?");
  await send(page, "random", "Plain update");
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveReply(grace, window.fake.messageWithText("Who reviews?"), "I can");
    window.fake.receiveReaction(grace, window.fake.messageWithText("Shipping notes are up"), "👍");
    window.fake.receiveMessage(grace, "general", "Not yours");
  });
  await openSent(page);

  const today = page.getByRole("region", { name: "Today" });
  const yesterday = page.getByRole("region", { name: "Yesterday" });
  await expect(today.getByRole("button", { name: /Shipping notes/ })).toHaveCount(0);
  await expect(today.getByRole("listitem")).toHaveCount(3);
  await expect(today.getByRole("listitem").first()).toContainText("Plain update");

  const plain = today.getByRole("button", { name: /Plain update/ });
  await expect(plain).not.toContainText("No replies yet");
  await expect(today.getByRole("button", { name: /check the build/ })).toContainText("No replies yet");
  const answered = today.getByRole("button", { name: /Who reviews/ });
  await expect(answered).toContainText("1 reply");
  await expect(answered).not.toContainText("No replies yet");
  await expect(yesterday.getByRole("button", { name: /Shipping notes/ })).toContainText("👍 1");

  await expect(page.getByRole("tabpanel")).not.toContainText("Not yours");
  await expect(page.getByRole("tabpanel")).not.toContainText("I can");
});

test("the filter narrows Sent to messages with those words", async ({ page }) => {
  await send(page, "general", "release notes are up");
  await send(page, "general", "lunch anyone?");
  await openSent(page);
  const panel = page.getByRole("tabpanel");
  await expect(panel.getByRole("listitem")).toHaveCount(2);

  await page.getByLabel("Filter sent messages").fill("rele");
  await expect(panel.getByRole("listitem")).toHaveCount(1);
  await expect(panel.getByRole("listitem")).toContainText("release notes are up");

  await page.getByLabel("Filter sent messages").fill("dinner");
  await expect(panel.getByText("Nothing you sent matches “dinner”.")).toBeVisible();
});

test("a sent message opens in context, in its thread if it's a reply", async ({ page }) => {
  await send(page, "general", "Who reviews?");
  await page.getByRole("article").filter({ hasText: "Who reviews?" }).hover();
  await page.getByRole("button", { name: "Reply in thread" }).click();
  const thread = page.getByRole("complementary", { name: "Thread" });
  await thread.getByLabel("Reply in thread").fill("I'll ask around");
  await thread.getByLabel("Reply in thread").press("Enter");
  await expect(thread.getByRole("article").filter({ hasText: "I'll ask around" })).toBeVisible();
  await openSent(page);

  const reply = page.getByRole("tabpanel").getByRole("button", { name: /I'll ask around/ });
  await expect(reply).toContainText("Thread in");
  await reply.click();
  await expect(thread.getByRole("article").filter({ hasText: "I'll ask around" })).toBeVisible();
});

test("a sent message's link can be copied and it can be saved for later", async ({ page, context, browserName }) => {
  await allowClipboard(context, browserName);
  await send(page, "general", "The plan is ready");
  await openSent(page);
  const row = page.getByRole("tabpanel").getByRole("listitem").filter({ hasText: "The plan is ready" });

  await row.hover();
  await row.getByRole("button", { name: "Sent message actions" }).click();
  await page.getByRole("menuitem", { name: "Copy link" }).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toMatch(/^shy:\/\/message\//);

  await row.hover();
  await row.getByRole("button", { name: "Sent message actions" }).click();
  await page.getByRole("menuitem", { name: "Save for later" }).click();
  await row.hover();
  await row.getByRole("button", { name: "Sent message actions" }).click();
  await expect(page.getByRole("menuitem", { name: "Remove from Later" })).toBeVisible();
});

test("Drafts & sent is a place in the history, and the app reopens there", async ({ page }) => {
  // The same seed gives the same ids, so the reloaded app is the same member.
  await page.goto("?seed=drafts-sent");
  await draftsAndSent(page).click();
  const heading = page.getByRole("heading", { name: "Drafts & sent" });
  await expect(heading).toBeVisible();
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.keyboard.press("Alt+ArrowLeft");
  await expect(heading).toBeVisible();
  await page.keyboard.press("Alt+ArrowRight");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Alt+ArrowLeft");
  await expect(heading).toBeVisible();

  await expect.poll(() => page.evaluate(() => localStorage.getItem("shyspace.lastView") ?? "")).toContain('"drafts"');
  await page.reload();
  await expect(heading).toBeVisible();
});
