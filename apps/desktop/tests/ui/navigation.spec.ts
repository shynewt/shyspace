import { expect, type Page, test } from "@playwright/test";

const MESSAGE_COUNT = 60;

function messages(page: Page, channel: string) {
  return page.getByRole("log", { name: `Messages in #${channel}` });
}

function sidebarChannel(page: Page, name: string) {
  return page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name, exact: true });
}

async function openChannel(page: Page, name: string) {
  await sidebarChannel(page, name).click();
  await expect(page.getByLabel(`Message #${name}`)).toBeVisible();
}

/** Grace writes "Message 1" to "Message 60" in #general; you've read them. */
async function fillGeneral(page: Page) {
  await page.evaluate((count) => {
    const grace = window.fake.addMember("Grace");
    for (let index = 1; index <= count; index += 1) window.fake.receiveMessage(grace, "general", `Message ${index}`);
    window.fake.handle("mark_all_read", undefined);
  }, MESSAGE_COUNT);
  await expect(messages(page, "general").getByText(`Message ${MESSAGE_COUNT}`, { exact: true })).toBeVisible();
}

async function createChannels(page: Page, names: string[]) {
  await page.evaluate((list) => {
    for (const name of list) window.fake.handle("create_channel", { name });
  }, names);
  for (const name of names) await expect(sidebarChannel(page, name)).toBeVisible();
}

/** Scrolls #general so this message's top sits a little below the list's top edge. */
async function scrollToMessage(page: Page, text: string) {
  await messages(page, "general").evaluate((log, wanted) => {
    const article = Array.from(log.querySelectorAll("article")).find((candidate) =>
      candidate.textContent?.includes(wanted),
    );
    if (!article) throw new Error(`no message ${wanted}`);
    log.scrollTop += article.getBoundingClientRect().top - log.getBoundingClientRect().top - 17;
  }, text);
}

/** How far the message's top sits below the top edge of #general's list. */
function offsetOf(page: Page, text: string): Promise<number> {
  return messages(page, "general").evaluate((log, wanted) => {
    const article = Array.from(log.querySelectorAll("article")).find((candidate) =>
      candidate.textContent?.includes(wanted),
    );
    if (!article) throw new Error(`no message ${wanted}`);
    return article.getBoundingClientRect().top - log.getBoundingClientRect().top;
  }, text);
}

/** Layout offsets are whole pixels, on-screen positions may not be. */
async function expectMessageAt(page: Page, text: string, offset: number) {
  await expect.poll(async () => Math.abs((await offsetOf(page, text)) - offset)).toBeLessThanOrEqual(1);
}

function pressMouseButton(page: Page, button: 3 | 4) {
  return page.getByLabel(/^Message #/).evaluate((composer, pressed) => {
    for (const type of ["mousedown", "mouseup", "auxclick"]) {
      composer.dispatchEvent(new MouseEvent(type, { button: pressed, bubbles: true, cancelable: true }));
    }
  }, button);
}

test.describe("in a session", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("back returns to the channel you were in, at the same message", async ({ page }) => {
    await fillGeneral(page);
    await createChannels(page, ["random"]);
    await scrollToMessage(page, "Message 20");
    await expectMessageAt(page, "Message 20", 17);

    await openChannel(page, "random");
    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByLabel("Message #general")).toBeVisible();
    await expectMessageAt(page, "Message 20", 17);

    await page.keyboard.press("Alt+ArrowRight");
    await expect(page.getByLabel("Message #random")).toBeVisible();
  });

  test("opening a channel you left scrolled up returns to where you were", async ({ page }) => {
    await fillGeneral(page);
    await createChannels(page, ["random"]);
    await scrollToMessage(page, "Message 30");

    await openChannel(page, "random");
    await openChannel(page, "general");
    await expectMessageAt(page, "Message 30", 17);
  });

  test("going somewhere new after going back clears what was ahead", async ({ page }) => {
    await createChannels(page, ["design", "random"]);
    await openChannel(page, "design");
    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByLabel("Message #general")).toBeVisible();

    await openChannel(page, "random");
    await page.keyboard.press("Alt+ArrowRight");
    await expect(page.getByLabel("Message #random")).toBeVisible();
    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("a deleted channel in history is skipped without a word", async ({ page }) => {
    await createChannels(page, ["design", "random"]);
    await openChannel(page, "design");
    await openChannel(page, "random");
    await page.evaluate(() => window.fake.handle("delete_channel", { channel: window.fake.channelId("design") }));
    await expect(sidebarChannel(page, "design")).toHaveCount(0);

    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByLabel("Message #general")).toBeVisible();
    await expect(page.getByText(/no longer open/)).toHaveCount(0);
  });

  test("going back says so when every earlier place was deleted", async ({ page }) => {
    await createChannels(page, ["design", "random"]);
    await openChannel(page, "design");
    await openChannel(page, "random");
    await page.evaluate(() => {
      window.fake.handle("delete_channel", { channel: window.fake.channelId("general") });
      window.fake.handle("delete_channel", { channel: window.fake.channelId("design") });
    });
    await expect(sidebarChannel(page, "general")).toHaveCount(0);

    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByText("The places before this were deleted or you can no longer open them.")).toBeVisible();
    await expect(page.getByLabel("Message #random")).toBeVisible();
  });

  test("settings is not a place in the history", async ({ page }) => {
    await createChannels(page, ["random"]);
    await openChannel(page, "random");
    await page.keyboard.press("Control+,");
    await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog", { name: "Settings" })).toHaveCount(0);

    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("the mouse's back and forward buttons work while you type", async ({ page }) => {
    await createChannels(page, ["random"]);
    await openChannel(page, "random");
    await page.getByLabel("Message #random").fill("half a thought");

    await pressMouseButton(page, 3);
    await expect(page.getByLabel("Message #general")).toBeVisible();
    await page.getByLabel("Message #general").focus();
    await pressMouseButton(page, 4);
    await expect(page.getByLabel("Message #random")).toHaveValue("half a thought");
  });

  test("in the page editor, back and forward keys stay the editor's", async ({ page }) => {
    await page.getByRole("button", { name: "New page" }).click();
    await page.getByLabel("Page title").fill("Notes");
    await page.getByLabel("Page title").press("Enter");
    await page.keyboard.type("some words");

    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByLabel("Page title")).toHaveValue("Notes");
    await page.getByLabel("Page title").focus();
    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("opening and closing a thread are steps back and forward", async ({ page }) => {
    await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "general", "Lunch plans?"));
    const thread = page.getByRole("complementary", { name: "Thread" });
    await messages(page, "general").getByRole("article").filter({ hasText: "Lunch plans?" }).hover();
    await page.getByRole("button", { name: "Reply in thread" }).click();
    await expect(thread).toBeVisible();
    await thread.getByRole("button", { name: "Close thread" }).click();
    await expect(thread).toHaveCount(0);

    await page.keyboard.press("Alt+ArrowLeft");
    await expect(thread).toBeVisible();
    await page.keyboard.press("Alt+ArrowLeft");
    await expect(thread).toHaveCount(0);
  });

  test("going back to a thread whose first message was deleted opens its channel", async ({ page }) => {
    await createChannels(page, ["random"]);
    const composer = page.getByLabel("Message #general");
    await composer.fill("Short-lived");
    await composer.press("Enter");
    await messages(page, "general").getByRole("article").filter({ hasText: "Short-lived" }).hover();
    await page.getByRole("button", { name: "Reply in thread" }).click();
    await expect(page.getByRole("complementary", { name: "Thread" })).toBeVisible();
    await openChannel(page, "random");
    await page.evaluate(() =>
      window.fake.handle("delete_message", { message: window.fake.messageWithText("Short-lived") }),
    );

    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByLabel("Message #general")).toBeVisible();
    await expect(page.getByRole("complementary", { name: "Thread" })).toHaveCount(0);
    await expect(page.getByRole("alert")).toHaveCount(0);
  });
});

test("launching reopens the last channel where you left it", async ({ page }) => {
  // The same seed gives the same ids, so the reloaded app finds the same channel and messages.
  await page.addInitScript((count) => {
    window.prepareFake = (fake) => {
      const grace = fake.addMember("Grace");
      for (let index = 1; index <= count; index += 1) fake.receiveMessage(grace, "general", `Message ${index}`);
      fake.handle("mark_all_read", undefined);
      fake.handle("create_channel", { name: "random" });
    };
  }, MESSAGE_COUNT);
  await page.goto("?seed=launch");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await scrollToMessage(page, "Message 25");
  // Message 24's end is still in view, so it's the top visible message.
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem("shyspace.lastView") ?? ""))
    .toContain(await page.evaluate(() => window.fake.messageWithText("Message 24")));

  await page.reload();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expectMessageAt(page, "Message 25", 17);
});
