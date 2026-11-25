/** An unsaved edit of your message waits as a draft of its own, like unsent text does. */

import { expect, type Page, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #random")).toBeVisible();
  await page.getByRole("button", { name: "general", exact: true }).click();
});

const composer = (page: Page) => page.getByLabel("Message #general");

async function sendAndStartEditing(page: Page, text: string) {
  await composer(page).fill(text);
  await composer(page).press("Enter");
  await expect(page.getByRole("article").filter({ hasText: text })).toBeVisible();
  await expect(composer(page)).toHaveValue("");
  await composer(page).press("ArrowUp");
  await expect(page.getByText("Editing message")).toBeVisible();
}

async function goAwayAndBack(page: Page) {
  await page.getByRole("button", { name: "random", exact: true }).click();
  await expect(page.getByLabel("Message #random")).toBeVisible();
  await sidebarChannel(page, /^general/).click();
}

function sidebarChannel(page: Page, name: RegExp) {
  return page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name });
}

function draftsList(page: Page) {
  return page.getByRole("list", { name: "Drafts" });
}

test("an unsaved edit comes back when you return, with the cursor at its end", async ({ page }) => {
  await sendAndStartEditing(page, "Lunch at noon");
  await composer(page).fill("Lunch at one");
  await goAwayAndBack(page);

  await expect(page.getByText("Editing message")).toBeVisible();
  await expect(composer(page)).toHaveValue("Lunch at one");
  await expect(composer(page)).toBeFocused();
  expect(await composer(page).evaluate((field: HTMLTextAreaElement) => field.selectionStart)).toBe(
    "Lunch at one".length,
  );

  await composer(page).press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "Lunch at one" })).toContainText("(edited)");
  await goAwayAndBack(page);
  await expect(page.getByText("Editing message")).toHaveCount(0);
  await expect(composer(page)).toHaveValue("");
});

test("cancelling an edit forgets it", async ({ page }) => {
  await sendAndStartEditing(page, "Lunch at noon");
  await composer(page).fill("Lunch at one");
  await goAwayAndBack(page);
  await expect(composer(page)).toHaveValue("Lunch at one");

  await composer(page).press("Escape");
  await goAwayAndBack(page);
  await expect(page.getByText("Editing message")).toHaveCount(0);
  await expect(page.getByRole("article").filter({ hasText: "Lunch at noon" })).not.toContainText("(edited)");
});

test("opening an edit without changing anything leaves nothing to resume", async ({ page }) => {
  await sendAndStartEditing(page, "Lunch at noon");
  await goAwayAndBack(page);
  await expect(composer(page)).toBeVisible();
  await expect(page.getByText("Editing message")).toHaveCount(0);
});

test("an edit and the conversation's draft both survive switching away", async ({ page }) => {
  await composer(page).fill("Lunch at noon");
  await composer(page).press("Enter");
  await composer(page).fill("a new thought");
  const message = page.getByRole("article").filter({ hasText: "Lunch at noon" });
  await message.hover();
  await page.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await composer(page).fill("Lunch at one");
  await goAwayAndBack(page);

  await expect(composer(page)).toHaveValue("Lunch at one");
  await composer(page).press("Escape");
  await expect(composer(page)).toHaveValue("a new thought");
});

test("Drafts & sent lists an unsaved edit and opens it to carry on", async ({ page }) => {
  await sendAndStartEditing(page, "Lunch at noon");
  await composer(page).fill("Lunch at one");
  await page.getByRole("button", { name: /^Drafts & sent/ }).click();

  const row = draftsList(page).getByRole("button", { name: /Lunch at one/ });
  await expect(row).toContainText("Editing a message in");
  await row.click();
  await expect(page.getByText("Editing message")).toBeVisible();
  await expect(composer(page)).toHaveValue("Lunch at one");
});

test("an unsaved edit is discarded from Drafts & sent, and Undo brings it back", async ({ page }) => {
  await sendAndStartEditing(page, "Lunch at noon");
  await composer(page).fill("Lunch at one");
  await page.getByRole("button", { name: /^Drafts & sent/ }).click();
  const row = draftsList(page).getByRole("listitem").filter({ hasText: "Lunch at one" });
  await row.hover();
  await row.getByRole("button", { name: "Discard draft" }).click();
  await expect(row).toHaveCount(0);

  await page.getByRole("button", { name: "Undo" }).click();
  await expect(row).toBeVisible();
});

test("the text of an edit whose message was deleted goes into the message box, after your draft", async ({ page }) => {
  await sendAndStartEditing(page, "Lunch at noon");
  await composer(page).fill("Lunch at one");
  await page.getByRole("button", { name: "random", exact: true }).click();
  await expect(page.getByLabel("Message #random")).toBeVisible();
  await page.evaluate(() => {
    const fake = window.fake;
    fake.handle("delete_message", { message: fake.messageWithText("Lunch at noon") });
    const channels = fake.handle("channels", undefined) as { id: string; name: string }[];
    const general = channels.find((channel) => channel.name === "general");
    if (!general) throw new Error("the fake has no #general");
    fake.handle("set_draft", { target: general.id, text: "a new thought" });
  });

  await page.getByRole("button", { name: /^Drafts & sent/ }).click();
  await expect(draftsList(page).getByRole("button", { name: /Lunch at one/ })).toContainText(
    "Edit of a deleted message in",
  );
  await sidebarChannel(page, /^general/).click();
  await expect(composer(page)).toHaveValue("a new thought\n\nLunch at one");
  await expect(page.getByText("Editing message")).toHaveCount(0);
  await expect(page.getByText("The message you were editing was deleted")).toBeVisible();
});

test("an unsaved edit in a thread comes back in that thread", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.receiveMessage(window.fake.addMember("Grace"), "general", "Who reviews?");
  });
  await page.getByRole("article").filter({ hasText: "Who reviews?" }).hover();
  await page.getByRole("button", { name: "Reply in thread" }).click();
  const thread = page.getByRole("complementary", { name: "Thread" });
  const reply = thread.getByLabel("Reply in thread");
  await reply.fill("I can, after lunch");
  await reply.press("Enter");
  await expect(reply).toHaveValue("");
  await reply.press("ArrowUp");
  await expect(reply).toHaveValue("I can, after lunch");
  await reply.fill("I can, after two");
  await page.getByRole("button", { name: /^Drafts & sent/ }).click();

  const row = draftsList(page).getByRole("button", { name: /I can, after two/ });
  await expect(row).toContainText("Editing a thread message in");
  await row.click();
  await expect(thread.getByText("Editing message")).toBeVisible();
  await expect(reply).toHaveValue("I can, after two");
});
