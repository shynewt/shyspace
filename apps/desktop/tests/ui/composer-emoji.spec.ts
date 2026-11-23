import { expect, type Page, test } from "@playwright/test";

function composer(page: Page) {
  return page.getByLabel("Message #general");
}

function emojiSuggestions(page: Page) {
  return page.getByRole("listbox", { name: "Emoji" });
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(composer(page)).toBeVisible();
});

test("the emoji button inserts at the cursor, replacing the selection", async ({ page }) => {
  await composer(page).fill("hello world");
  await composer(page).evaluate((element: HTMLTextAreaElement) => element.setSelectionRange(6, 11));
  await page.getByRole("button", { name: "Insert emoji" }).first().click();
  await page.getByLabel("Search emoji").fill("tada");
  await page.getByRole("gridcell", { name: "Party popper" }).click();

  await expect(composer(page)).toHaveValue("hello 🎉");
  await expect(composer(page)).toBeFocused();
  await page.keyboard.type("!");
  await expect(composer(page)).toHaveValue("hello 🎉!");
  await expect
    .poll(() => page.evaluate(() => window.fake.handle("drafts", undefined)))
    .toContainEqual(expect.objectContaining({ text: "hello 🎉!" }));
});

test("a search typed while the emoji are still loading applies once they're there", async ({ page }) => {
  let release = () => {};
  const loaded = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route(/emojibase-data/, async (route) => {
    await loaded;
    await route.continue();
  });
  await page.getByRole("button", { name: "Insert emoji" }).first().click();
  await expect(page.getByText("Loading emoji…")).toBeVisible();
  await page.getByLabel("Search emoji").fill("rocket");

  release();
  await page.getByRole("gridcell", { name: "Rocket" }).click();
  await expect(composer(page)).toHaveValue("🚀");
});

test("thread replies have the emoji button too", async ({ page }) => {
  await composer(page).fill("Release today?");
  await composer(page).press("Enter");
  await page.getByRole("log", { name: "Messages in #general" }).getByRole("article").hover();
  await page.getByRole("button", { name: "Reply in thread" }).click();
  const thread = page.getByRole("complementary", { name: "Thread" });

  await thread.getByRole("button", { name: "Insert emoji" }).click();
  await page.getByLabel("Search emoji").fill("rocket");
  await page.getByRole("gridcell", { name: "Rocket" }).click();
  await expect(thread.getByRole("textbox")).toHaveValue("🚀");
});

test("more emoji reacts with any emoji, and again takes it back", async ({ page }) => {
  await composer(page).fill("Alpha");
  await composer(page).press("Enter");
  const message = page.getByRole("log", { name: "Messages in #general" }).getByRole("article").filter({
    hasText: "Alpha",
  });
  const reactWith = async (search: string, label: string) => {
    await message.hover();
    await message.getByRole("button", { name: "React", exact: true }).click();
    await page.getByRole("button", { name: "More emoji…" }).click();
    await page.getByLabel("Search emoji").fill(search);
    await page.getByRole("gridcell", { name: label }).click();
  };

  await reactWith("palm", "Palm tree");
  await expect(message.getByRole("button", { name: "🌴 1" })).toBeVisible();
  await reactWith("palm", "Palm tree");
  await expect(message.getByRole("button", { name: "🌴 1" })).toHaveCount(0);
});

test("typing a colon and a name suggests emoji, picked with the keyboard", async ({ page }) => {
  await composer(page).fill("so :partying");
  const suggestions = emojiSuggestions(page);
  await expect(suggestions.getByRole("option", { name: /Partying face/ })).toBeVisible();
  await composer(page).press("Enter");
  await expect(composer(page)).toHaveValue("so 🥳");
  await expect(suggestions).toHaveCount(0);

  await composer(page).fill("go :rocket");
  await expect(suggestions.getByRole("option", { name: /Rocket/ })).toBeVisible();
  await composer(page).press("Tab");
  await expect(composer(page)).toHaveValue("go 🚀");

  await composer(page).fill("hmm :thumbs");
  await expect(suggestions).toBeVisible();
  await composer(page).press("Escape");
  await expect(suggestions).toHaveCount(0);
  await expect(composer(page)).toHaveValue("hmm :thumbs");
});

test("the colon suggestions find emoji by shortcode and keyword", async ({ page }) => {
  await composer(page).fill(":tada");
  await expect(emojiSuggestions(page).getByRole("option", { name: /Party popper/ })).toBeVisible();
  await composer(page).fill(":woohoo");
  await expect(emojiSuggestions(page).getByRole("option", { name: /Party popper/ })).toBeVisible();
});

test("recently used emoji come first in the colon suggestions", async ({ page }) => {
  await composer(page).fill(":tada");
  await expect(emojiSuggestions(page).getByRole("option", { name: /Party popper/ })).toBeVisible();
  await composer(page).press("Enter");

  await composer(page).fill("x :pa");
  await expect(emojiSuggestions(page).getByRole("option").first()).toHaveAccessibleName(/Party popper/);
});

test("the colon suggestions stay away from code, links, times and single letters", async ({ page }) => {
  await composer(page).fill("warm up :party");
  await expect(emojiSuggestions(page)).toBeVisible();

  for (const text of ["`code :party", "see https://party.example", "at 10:30", "ok :p", "a:party"]) {
    await composer(page).fill(text);
    await expect(emojiSuggestions(page), text).toHaveCount(0);
  }
});

test("emoji data loads only once a colon search starts", async ({ page }) => {
  const emojiRequests: string[] = [];
  page.on("request", (request) => {
    if (/emojibase-data|frimousse|EmojiPickerPanel/.test(request.url())) emojiRequests.push(request.url());
  });
  await composer(page).fill("hello there");
  await page.waitForTimeout(300);
  expect(emojiRequests).toEqual([]);

  await composer(page).fill("hello :wave");
  await expect(emojiSuggestions(page)).toBeVisible();
  expect(emojiRequests.some((url) => url.includes("emojibase-data"))).toBe(true);
});

/** Picks with a click and reads the box in the same moment: a key typed right after must land after the emoji. */
async function textAndCaretAfterClicking(page: Page, cell: string) {
  return page.getByRole("gridcell", { name: cell }).evaluate((element: HTMLElement) => {
    element.click();
    const box = document.activeElement;
    return box instanceof HTMLTextAreaElement ? [box.value, box.selectionStart] : null;
  });
}

test("the caret is right after a picked emoji at once", async ({ page }) => {
  await composer(page).fill("ab");
  await composer(page).evaluate((element: HTMLTextAreaElement) => element.setSelectionRange(1, 1));
  await page.getByRole("button", { name: "Insert emoji" }).first().click();
  await page.getByLabel("Search emoji").fill("tada");
  await expect(page.getByRole("gridcell", { name: "Party popper" })).toBeVisible();

  expect(await textAndCaretAfterClicking(page, "Party popper")).toEqual(["a🎉b", 3]);
});

test("the caret is right after an emoji chosen from suggestions at once", async ({ page }) => {
  await composer(page).fill("x");
  await composer(page).pressSequentially(" :tada");
  await expect(emojiSuggestions(page)).toBeVisible();

  const result = await composer(page).evaluate((element: HTMLTextAreaElement) => {
    element.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    return [element.value, element.selectionStart];
  });
  expect(result).toEqual(["x 🎉", 4]);
});
