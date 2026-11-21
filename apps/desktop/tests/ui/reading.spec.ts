/** What counts as read, the new-messages line, the two pills, and keeping the view still. */
import { expect, type Page, test } from "@playwright/test";
import type { Channel } from "../../src/lib/api";

function log(page: Page, channel = "general") {
  return page.getByRole("log", { name: `Messages in #${channel}` });
}

function sidebarChannel(page: Page, name: RegExp | string) {
  return page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name });
}

const unreadLine = (page: Page) => page.getByRole("separator", { name: "New messages" });
const jumpToUnread = (page: Page) => page.getByRole("button", { name: "Jump to unread" });
const newMessagesPill = (page: Page) => page.getByRole("button", { name: /new messages?$/ });

/** The window loses or gets focus, as when you switch apps. */
async function setWindowFocused(page: Page, isFocused: boolean) {
  await page.evaluate((focused) => {
    Object.defineProperty(document, "hasFocus", { configurable: true, value: () => focused });
    window.dispatchEvent(new Event(focused ? "focus" : "blur"));
  }, isFocused);
}

/** The window is minimised or on another workspace, or comes back. */
async function setWindowHidden(page: Page, isHidden: boolean) {
  await page.evaluate((hidden) => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => (hidden ? "hidden" : "visible"),
    });
    Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
    document.dispatchEvent(new Event("visibilitychange"));
  }, isHidden);
}

/** Unread counts as the core keeps them, by channel name. */
function recount(page: Page): Promise<Record<string, number>> {
  return page.evaluate(() => {
    const channels = window.fake.handle("channels", undefined) as Channel[];
    return Object.fromEntries(channels.map((channel) => [channel.name, channel.unread]));
  });
}

async function unreadIn(page: Page, channel: string): Promise<number> {
  return (await recount(page))[channel];
}

/** Waits longer than reading takes to be saved (about 300 ms). */
function afterSaving(page: Page) {
  return page.waitForTimeout(700);
}

async function graceSays(page: Page, channel: string, ...texts: string[]) {
  await page.evaluate(
    ({ channel, texts }) => {
      const grace = (() => {
        try {
          return window.fake.memberNamed("Grace");
        } catch {
          return window.fake.addMember("Grace");
        }
      })();
      for (const text of texts) window.fake.receiveMessage(grace, channel, text);
    },
    { channel, texts },
  );
}

/** #general with "Old 1" up to "Old <count>", all read, showing the newest. */
async function readHistory(page: Page, count = 60) {
  await page.evaluate((total) => {
    const grace = window.fake.addMember("Grace");
    for (let index = 1; index <= total; index += 1) window.fake.receiveMessage(grace, "general", `Old ${index}`);
    window.fake.handle("mark_all_read", undefined);
  }, count);
  await expect(log(page).getByText(`Old ${count}`, { exact: true })).toBeInViewport();
}

/** Turns the wheel up over a list and waits until the list has moved: wheel events arrive on their own schedule. */
async function wheelUp(page: Page, channel: string, pixels: number) {
  await log(page, channel).hover();
  await page.mouse.wheel(0, -pixels);
  await expect
    .poll(() =>
      log(page, channel).evaluate((element) => element.scrollHeight - element.scrollTop - element.clientHeight),
    )
    .toBeGreaterThan(0);
}

/** Scrolls #general up by a person's wheel until "Old <n>" is at the top. */
async function scrollUpTo(page: Page, text: string) {
  await wheelUp(page, "general", 200);
  await log(page).evaluate((element, wanted) => {
    const article = Array.from(element.querySelectorAll("article")).find((candidate) =>
      candidate.textContent?.includes(wanted),
    );
    if (!article) throw new Error(`no message ${wanted}`);
    element.scrollTop += article.getBoundingClientRect().top - element.getBoundingClientRect().top - 9;
  }, text);
  await expect(log(page).getByText(text, { exact: true })).toBeInViewport();
}

function topOf(page: Page, text: string): Promise<number> {
  return log(page).evaluate((element, wanted) => {
    const article = Array.from(element.querySelectorAll("article")).find((candidate) =>
      candidate.textContent?.includes(wanted),
    );
    if (!article) throw new Error(`no message ${wanted}`);
    return article.getBoundingClientRect().top - element.getBoundingClientRect().top;
  }, text);
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a message that arrives while the window is unfocused stays unread, even on screen", async ({ page }) => {
  await readHistory(page, 5);
  await setWindowFocused(page, false);
  await graceSays(page, "general", "Are you there?");

  await expect(log(page).getByText("Are you there?")).toBeInViewport();
  await afterSaving(page);
  expect(await unreadIn(page, "general")).toBe(1);
  await expect(unreadLine(page)).toBeVisible();
});

test("focusing the window marks what's on screen read, and the line stays", async ({ page }) => {
  await readHistory(page, 5);
  await setWindowFocused(page, false);
  await graceSays(page, "general", "Are you there?");
  await expect(unreadLine(page)).toBeVisible();

  await setWindowFocused(page, true);
  await expect.poll(() => unreadIn(page, "general")).toBe(0);
  await expect(unreadLine(page)).toBeVisible();
});

test("focusing the window marks only the messages on screen", async ({ page }) => {
  await readHistory(page);
  await scrollUpTo(page, "Old 20");
  await setWindowFocused(page, false);
  await graceSays(page, "general", "New 1", "New 2", "New 3");

  await setWindowFocused(page, true);
  await afterSaving(page);
  expect(await unreadIn(page, "general")).toBe(3);
  await expect(newMessagesPill(page)).toHaveAccessibleName("3 new messages");
});

test("a hidden window never moves the read position", async ({ page }) => {
  await readHistory(page, 5);
  await setWindowHidden(page, true);
  await graceSays(page, "general", "While you were away");
  await afterSaving(page);
  expect(await unreadIn(page, "general")).toBe(1);

  await setWindowHidden(page, false);
  await expect.poll(() => unreadIn(page, "general")).toBe(0);
});

test("while unfocused, the view keeps following new messages at the bottom", async ({ page }) => {
  await readHistory(page);
  await setWindowFocused(page, false);
  await graceSays(page, "general", "Still following");

  await expect(log(page).getByText("Still following")).toBeInViewport();
  await expect(newMessagesPill(page)).toHaveCount(0);
});

test("ten fast channel switches leave the counts equal to a recount", async ({ page }) => {
  await page.evaluate(() => {
    for (const name of ["alpha", "beta", "gamma"]) window.fake.handle("create_channel", { name });
    const grace = window.fake.addMember("Grace");
    for (const name of ["alpha", "beta", "gamma"]) {
      for (let index = 1; index <= 40; index += 1) window.fake.receiveMessage(grace, name, `${name} ${index}`);
    }
  });
  await expect(sidebarChannel(page, /^gamma, 40 unread/)).toBeVisible();

  for (const name of ["alpha", "beta", "gamma", "general", "alpha", "gamma", "beta", "general", "alpha", "beta"]) {
    await sidebarChannel(page, new RegExp(`^${name}\\b`)).click();
  }
  await expect(page.getByLabel("Message #beta")).toBeVisible();
  await afterSaving(page);

  const counts = await recount(page);
  for (const [name, unread] of Object.entries(counts)) {
    const shown = sidebarChannel(page, new RegExp(`^${name}\\b`));
    if (unread === 0) await expect(shown).toHaveAccessibleName(name);
    else await expect(shown).toHaveAccessibleName(new RegExp(`^${name}, ${unread} unread`));
  }
});

test("unread you read or cleared is saved before you leave, so a restart can't bring it back", async ({ page }) => {
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  await graceSays(page, "random", "One", "Two");
  await sidebarChannel(page, /^random, 2 unread/).click();
  await expect(log(page, "random").getByText("Two")).toBeInViewport();
  await sidebarChannel(page, /^general\b/).click();
  expect(await unreadIn(page, "random")).toBe(0);

  await graceSays(page, "random", "Three");
  await sidebarChannel(page, /^random, 1 unread/).click();
  await expect(log(page, "random").getByText("Three")).toBeInViewport();
  await setWindowFocused(page, false);
  await expect.poll(() => unreadIn(page, "random"), { timeout: 200 }).toBe(0);

  await setWindowFocused(page, true);
  await graceSays(page, "random", "Four");
  await setWindowFocused(page, false);
  await graceSays(page, "random", "Five");
  await page.keyboard.press("Escape");
  await expect.poll(() => unreadIn(page, "random"), { timeout: 200 }).toBe(0);
});

test("coming straight back to a channel you just read shows no new-messages line", async ({ page }) => {
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  await graceSays(page, "random", "One", "Two");
  await sidebarChannel(page, /^random, 2 unread/).click();
  await expect(log(page, "random").getByText("Two")).toBeInViewport();
  await sidebarChannel(page, /^general\b/).click();
  await sidebarChannel(page, /^random\b/).click();

  await expect(log(page, "random").getByText("Two")).toBeInViewport();
  await page.waitForTimeout(300);
  await expect(unreadLine(page)).toHaveCount(0);
});

test("jump to unread never shows when the first unread message is in view", async ({ page }) => {
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  await graceSays(page, "random", "One", "Two", "Three");
  await sidebarChannel(page, /^random, 3 unread/).click();

  await expect(unreadLine(page)).toBeInViewport();
  await page.waitForTimeout(300);
  await expect(jumpToUnread(page)).toHaveCount(0);
});

test("jump to unread shows when the first unread is above, and takes you there once", async ({ page }) => {
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  const texts = Array.from({ length: 40 }, (_, index) => `Unread ${index + 1}`);
  await graceSays(page, "random", ...texts);
  const newest = await page.evaluate(() => window.fake.messageWithText("Unread 40"));
  await page.getByLabel("Message #general").fill(`see shy://message/${newest}`);
  await page.getByLabel("Message #general").press("Enter");
  await page.getByRole("button", { name: "a message" }).click();
  await expect(page.getByLabel("Message #random")).toBeVisible();

  await expect(jumpToUnread(page)).toBeVisible();
  await jumpToUnread(page).click();
  await expect(unreadLine(page)).toBeInViewport();
  await expect(jumpToUnread(page)).toHaveCount(0);
  await log(page, "random").evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await page.waitForTimeout(300);
  await expect(jumpToUnread(page)).toHaveCount(0);
});

test("new messages while you read history count up in a pill that takes you down", async ({ page }) => {
  await readHistory(page);
  await scrollUpTo(page, "Old 20");
  await graceSays(page, "general", "Fresh 1");
  await expect(newMessagesPill(page)).toHaveAccessibleName("1 new message");
  await graceSays(page, "general", "Fresh 2");
  await expect(newMessagesPill(page)).toHaveAccessibleName("2 new messages");
  await afterSaving(page);
  expect(await unreadIn(page, "general")).toBe(2);

  await newMessagesPill(page).click();
  await expect(log(page).getByText("Fresh 2")).toBeInViewport();
  await expect(newMessagesPill(page)).toHaveCount(0);
  await expect.poll(() => unreadIn(page, "general")).toBe(0);
});

test("sending your own message takes you down and hides the pill", async ({ page }) => {
  await readHistory(page);
  await scrollUpTo(page, "Old 20");
  await graceSays(page, "general", "Fresh 1");
  await expect(newMessagesPill(page)).toBeVisible();

  await page.getByLabel("Message #general").fill("On it");
  await page.getByLabel("Message #general").press("Enter");
  await expect(log(page).getByText("On it")).toBeInViewport();
  await expect(newMessagesPill(page)).toHaveCount(0);
});

test("sending keeps you at the newest message with a table further up", async ({ page }) => {
  await readHistory(page, 20);
  await graceSays(page, "general", "| Area | Merged |\n| --- | ---: |\n| Sync | 6 |\n| Pages | 5 |\n| Desktop | 3 |");
  const composer = page.getByLabel("Message #general");
  for (const text of ["One", "Two", "Three"]) {
    await composer.fill(text);
    await composer.press("Enter");
    await expect(log(page).getByText(text, { exact: true })).toBeInViewport();
    await page.waitForTimeout(300);
    const distance = await log(page).evaluate(
      (element) => element.scrollHeight - element.scrollTop - element.clientHeight,
    );
    expect(distance, `after sending "${text}"`).toBeLessThanOrEqual(1);
  }
});

test("a message arriving while you read history moves the view 0px", async ({ page }) => {
  await readHistory(page);
  await scrollUpTo(page, "Old 20");
  const before = await topOf(page, "Old 20");

  await graceSays(page, "general", "Fresh 1");
  await expect(newMessagesPill(page)).toBeVisible();
  expect(await topOf(page, "Old 20")).toBe(before);
  await page.waitForTimeout(400);
  expect(await topOf(page, "Old 20")).toBe(before);
});

test("an upward wheel stops following new messages at once", async ({ page }) => {
  await readHistory(page);
  await wheelUp(page, "general", 10);
  await graceSays(page, "general", "Below the fold?");

  await expect(newMessagesPill(page)).toBeVisible();
});

test("Esc marks everything read and clears the line and both pills", async ({ page }) => {
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  const texts = Array.from({ length: 40 }, (_, index) => `Unread ${index + 1}`);
  await graceSays(page, "random", ...texts);
  const middle = await page.evaluate(() => window.fake.messageWithText("Unread 30"));
  await page.getByLabel("Message #general").fill(`see shy://message/${middle}`);
  await page.getByLabel("Message #general").press("Enter");
  await page.getByRole("button", { name: "a message" }).click();
  await expect(jumpToUnread(page)).toBeVisible();
  await wheelUp(page, "random", 50);
  await graceSays(page, "random", "Even newer");
  await expect(newMessagesPill(page)).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(unreadLine(page)).toHaveCount(0);
  await expect(jumpToUnread(page)).toHaveCount(0);
  await expect(newMessagesPill(page)).toHaveCount(0);
  await expect(log(page, "random").getByText("Even newer")).toBeInViewport();
  await expect.poll(() => unreadIn(page, "random")).toBe(0);
});

test("a thread's unread replies never change the channel's unread", async ({ page }) => {
  await readHistory(page);
  await page.getByLabel("Message #general").fill("Lunch?");
  await page.getByLabel("Message #general").press("Enter");
  await expect(log(page).getByText("Lunch?")).toBeInViewport();
  await page.evaluate(() =>
    window.fake.receiveReply(window.fake.memberNamed("Grace"), window.fake.messageWithText("Lunch?"), "Sure"),
  );
  await expect(log(page).getByRole("button", { name: /1 reply.*1 new/ })).toBeVisible();
  await afterSaving(page);
  expect(await unreadIn(page, "general")).toBe(0);

  await log(page)
    .getByRole("button", { name: /1 reply.*1 new/ })
    .click();
  const thread = page.getByRole("complementary", { name: "Thread" });
  await expect(thread.getByText("Sure")).toBeVisible();
  await expect(log(page).getByRole("button", { name: /new/ })).toHaveCount(0);
  await scrollUpTo(page, "Old 20");
  await page.evaluate(() => {
    const grace = window.fake.memberNamed("Grace");
    window.fake.receiveMessage(grace, "general", "Unseen in the channel");
    window.fake.receiveReply(grace, window.fake.messageWithText("Lunch?"), "Seen in the thread");
  });
  await expect(thread.getByText("Seen in the thread")).toBeInViewport();
  await afterSaving(page);

  expect(await unreadIn(page, "general")).toBe(1);
  await expect(log(page).getByRole("button", { name: /new/ })).toHaveCount(0);
});

test("a wheel turn in a channel too short to scroll changes nothing", async ({ page }) => {
  await readHistory(page, 3);
  await log(page).hover();
  await page.mouse.wheel(0, -40);
  await page.waitForTimeout(300);
  await graceSays(page, "general", "Short and sweet");

  await expect(log(page).getByText("Short and sweet")).toBeInViewport();
  await page.waitForTimeout(300);
  await expect(newMessagesPill(page)).toHaveCount(0);
});

test("scrolling back down follows new messages again only right at the bottom", async ({ page }) => {
  await readHistory(page);
  await wheelUp(page, "general", 200);
  await log(page).evaluate((element) => {
    element.scrollTop = element.scrollHeight - element.clientHeight - 40;
  });
  await page.waitForTimeout(100);
  await graceSays(page, "general", "Not followed");
  await expect(newMessagesPill(page)).toBeVisible();

  await log(page).evaluate((element) => {
    element.scrollTop = element.scrollHeight - element.clientHeight - 10;
  });
  await expect(newMessagesPill(page)).toHaveCount(0);
  await graceSays(page, "general", "Followed");
  await expect(log(page).getByText("Followed", { exact: true })).toBeInViewport();
});

test("jump to unread doesn't appear later in a visit, after reading a long first message", async ({ page }) => {
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  const long = Array.from({ length: 60 }, (_, index) => `line ${index + 1}`).join("\n");
  await graceSays(page, "random", long, "After the long one");
  await sidebarChannel(page, /^random, 2 unread/).click();
  await expect(unreadLine(page)).toBeInViewport();
  await page.waitForTimeout(300);

  await log(page, "random").evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect(log(page, "random").getByText("After the long one")).toBeInViewport();
  await page.waitForTimeout(500);
  await expect(jumpToUnread(page)).toHaveCount(0);
});

test("a message arriving unfocused moves the line once everything below it was read", async ({ page }) => {
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  await graceSays(page, "random", "First batch");
  await sidebarChannel(page, /^random, 1 unread/).click();
  await expect(unreadLine(page)).toBeVisible();
  await expect.poll(() => unreadIn(page, "random")).toBe(0);

  await setWindowFocused(page, false);
  await graceSays(page, "random", "Second batch");
  const lineThenMessage = log(page, "random").locator("[role=separator] + div", { hasText: "Second batch" });
  await expect(lineThenMessage).toHaveCount(1);
});

test("a message deleted just after you read it doesn't leave unread behind", async ({ page }) => {
  await readHistory(page, 3);
  await graceSays(page, "general", "Keep", "Gone soon");
  await expect(log(page).getByText("Gone soon")).toBeInViewport();
  await page.evaluate(() =>
    window.fake.handle("delete_message", { message: window.fake.messageWithText("Gone soon") }),
  );

  await expect.poll(() => unreadIn(page, "general")).toBe(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("opening a link to an older message doesn't mark the unread ones below as read", async ({ page }) => {
  await readHistory(page);
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  await sidebarChannel(page, /^random\b/).click();
  await graceSays(page, "general", "Unread 1", "Unread 2");
  const old = await page.evaluate(() => window.fake.messageWithText("Old 2"));
  await page.getByLabel("Message #random").fill(`see shy://message/${old}`);
  await page.getByLabel("Message #random").press("Enter");
  await page.getByRole("button", { name: "a message" }).click();
  await expect(log(page).getByText("Old 2", { exact: true })).toBeInViewport();

  await afterSaving(page);
  expect(await unreadIn(page, "general")).toBe(2);
});

test("Esc in another field doesn't mark the channel read", async ({ page }) => {
  await readHistory(page);
  await scrollUpTo(page, "Old 20");
  await graceSays(page, "general", "Below");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("draft");
  await page.getByLabel("Channel name").press("Escape");
  await expect(page.getByLabel("Channel name")).toHaveCount(0);

  await afterSaving(page);
  expect(await unreadIn(page, "general")).toBe(1);
  await expect(newMessagesPill(page)).toBeVisible();
});

test("a pill puts keyboard focus on the messages after it goes", async ({ page }) => {
  await readHistory(page);
  await scrollUpTo(page, "Old 20");
  await graceSays(page, "general", "Fresh");
  await newMessagesPill(page).focus();
  await page.keyboard.press("Enter");

  await expect(newMessagesPill(page)).toHaveCount(0);
  await expect(log(page)).toBeFocused();
});

test("the shortcuts sheet says what Esc does in a channel", async ({ page }) => {
  await page.keyboard.press("Control+/");
  await expect(page.getByRole("dialog", { name: "Keyboard shortcuts" })).toContainText(
    "Mark the channel read and jump to the newest message",
  );
});
