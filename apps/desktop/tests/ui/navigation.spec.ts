import { expect, type Page, test } from "@playwright/test";

const MESSAGE_COUNT = 60;

declare global {
  interface Window {
    /** Lets the held first load of messages through. */
    releaseMessages?: () => void;
  }
}

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

/** Grace writes "Message 1" up to "Message <count>" in #general; you've read them. */
async function fillGeneral(page: Page, count = MESSAGE_COUNT) {
  await page.evaluate((total) => {
    const grace = window.fake.addMember("Grace");
    for (let index = 1; index <= total; index += 1) window.fake.receiveMessage(grace, "general", `Message ${index}`);
    window.fake.handle("mark_all_read", undefined);
  }, count);
  await expect(messages(page, "general").getByText(`Message ${count}`, { exact: true })).toBeVisible();
}

/** Scrolls to the top until the first message has loaded, as a reader going back in time does. */
async function scrollToFirstMessage(page: Page) {
  const log = messages(page, "general");
  await expect(async () => {
    await log.evaluate((element) => {
      element.scrollTop = 0;
    });
    await expect(log.getByText("Message 1", { exact: true })).toBeAttached({ timeout: 500 });
  }).toPass();
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
    // Messages added before #general has loaded would arrive as unread history.
    await expect(messages(page, "general")).toHaveAttribute("aria-busy", "false");
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

  test("loading older messages keeps the message at the top where it was", async ({ page }) => {
    await fillGeneral(page, 150);
    // Without the browser's own scroll anchoring, as in WebKit.
    await messages(page, "general").evaluate((log) => {
      log.style.overflowAnchor = "none";
    });
    await scrollToMessage(page, "Message 60");
    await messages(page, "general").evaluate((log) => {
      log.scrollTop = 30;
    });
    const before = await offsetOf(page, "Message 60");

    await expect(messages(page, "general").getByText("Message 1", { exact: true })).toBeAttached();
    await expectMessageAt(page, "Message 60", before);
  });

  test("back returns to a message older than the first hundred", async ({ page }) => {
    await fillGeneral(page, 150);
    await createChannels(page, ["random"]);
    await scrollToFirstMessage(page);
    await scrollToMessage(page, "Message 10");

    await openChannel(page, "random");
    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByLabel("Message #general")).toBeVisible();
    await expectMessageAt(page, "Message 10", 17);
  });

  test("back to a place whose message was deleted opens the channel at the newest message", async ({ page }) => {
    await fillGeneral(page);
    await createChannels(page, ["random"]);
    await scrollToMessage(page, "Message 20");
    await openChannel(page, "random");
    await page.evaluate(() => {
      for (const text of ["Message 19", "Message 20"]) {
        window.fake.handle("delete_message", { message: window.fake.messageWithText(text) });
      }
    });

    await page.keyboard.press("Alt+ArrowLeft");
    await expect(messages(page, "general").getByText("Message 60", { exact: true })).toBeInViewport();
  });

  test("a link to a message older than the first hundred loads history until it's there", async ({ page }) => {
    await fillGeneral(page, 150);
    await createChannels(page, ["random"]);
    await openChannel(page, "random");
    const id = await page.evaluate(() => window.fake.messageWithText("Message 3"));
    await page.getByLabel("Message #random").fill(`see shy://message/${id}`);
    await page.getByLabel("Message #random").press("Enter");

    await page.getByRole("button", { name: "a message" }).click();
    await expect(page.getByLabel("Message #general")).toBeVisible();
    await expect(messages(page, "general").getByText("Message 3", { exact: true })).toBeInViewport();
  });

  test("back to a place you left at the newest message returns there", async ({ page }) => {
    await fillGeneral(page);
    const thread = page.getByRole("complementary", { name: "Thread" });
    const last = messages(page, "general").getByRole("article").filter({ hasText: "Message 60" });
    await last.hover();
    await last.getByRole("button", { name: "Reply in thread" }).click();
    await expect(thread).toBeVisible();
    await scrollToMessage(page, "Message 20");

    await page.keyboard.press("Alt+ArrowLeft");
    await expect(thread).toHaveCount(0);
    await expect(messages(page, "general").getByText("Message 60", { exact: true })).toBeInViewport();
  });

  test("a channel with something new opens at the new messages, not where you left it", async ({ page }) => {
    await fillGeneral(page);
    await createChannels(page, ["random"]);
    await scrollToMessage(page, "Message 20");
    await openChannel(page, "random");
    await page.evaluate(() => window.fake.receiveMessage(window.fake.memberNamed("Grace"), "general", "Fresh news"));

    await page.getByRole("button", { name: "general, 1 unread" }).click();
    await expect(page.getByRole("separator", { name: "New messages" })).toBeInViewport();
    await expect(messages(page, "general").getByText("Fresh news")).toBeInViewport();
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

test("back and forward move between a channel and a DM", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  const dm = await page.evaluate(() => window.fake.handle("open_dm", { members: [window.fake.addMember("Grace")] }));
  expect(dm).toMatch(/^dm:/);
  await page
    .getByRole("region", { name: "Direct messages" })
    .getByRole("button", { name: /^Grace, direct message/ })
    .click();
  await expect(page.getByLabel("Message Grace")).toBeVisible();

  await page.keyboard.press("Alt+ArrowLeft");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Alt+ArrowRight");
  await expect(page.getByLabel("Message Grace")).toBeVisible();
});

test("launching reopens a DM", async ({ page }) => {
  await page.addInitScript(() => {
    window.prepareFake = (fake) => {
      const dm = fake.handle("open_dm", { members: [fake.addMember("Grace")] });
      if (typeof dm === "string") fake.receiveMessageIn(fake.memberNamed("Grace"), dm, "Psst");
    };
  });
  await page.goto("?seed=dm");
  await page
    .getByRole("region", { name: "Direct messages" })
    .getByRole("button", { name: /^Grace, direct message/ })
    .click();
  await expect(page.getByLabel("Message Grace")).toBeVisible();
  await expect.poll(() => page.evaluate(() => localStorage.getItem("shyspace.lastView.w-acme") ?? "")).toContain("dm:");

  await page.reload();
  await expect(page.getByLabel("Message Grace")).toBeVisible();
  await expect(page.getByText("Psst")).toBeVisible();
});

test("a private channel you were removed from is skipped in history", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() => {
    window.fake.receivePrivateChannel(window.fake.addMember("Grace"), "ops");
    window.fake.handle("create_channel", { name: "random" });
  });
  await page
    .getByRole("navigation", { name: "Workspace" })
    .getByRole("button", { name: /^ops, private channel/ })
    .click();
  await expect(page.getByLabel("Message #ops")).toBeVisible();
  await openChannel(page, "random");
  await page.evaluate(() => window.fake.removeMeFromPrivateChannel("ops"));

  await page.keyboard.press("Alt+ArrowLeft");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("launching reopens the thread you had open", async ({ page }) => {
  await page.addInitScript(() => {
    window.prepareFake = (fake) => {
      const grace = fake.addMember("Grace");
      const root = fake.receiveMessage(grace, "general", "Plans for the offsite");
      fake.receiveReply(grace, root, "Lisbon?");
    };
  });
  await page.goto("?seed=thread");
  await messages(page, "general")
    .getByRole("button", { name: /1 reply/ })
    .click();
  const thread = page.getByRole("complementary", { name: "Thread" });
  await expect(thread.getByText("Lisbon?")).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem("shyspace.lastView.w-acme") ?? ""))
    .toContain("thread");

  await page.reload();
  await expect(thread.getByText("Lisbon?")).toBeVisible();
});

test("a channel's messages read as loading until they're there", async ({ page }) => {
  await page.addInitScript(() => {
    window.prepareFake = (fake) => {
      const answer = fake.handle.bind(fake);
      let release = () => {};
      const loaded = new Promise<void>((resolve) => {
        release = resolve;
      });
      window.releaseMessages = release;
      fake.handle = (command, args) =>
        command === "messages" ? loaded.then(() => answer(command, args)) : answer(command, args);
    };
  });
  await page.goto("");
  const log = messages(page, "general");
  await expect(log).toHaveAttribute("aria-busy", "true");

  await page.evaluate(() => window.releaseMessages?.());
  await expect(log).toHaveAttribute("aria-busy", "false");
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
    .poll(() => page.evaluate(() => localStorage.getItem("shyspace.lastView.w-acme") ?? ""))
    .toContain(await page.evaluate(() => window.fake.messageWithText("Message 24")));

  await page.reload();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expectMessageAt(page, "Message 25", 17);
});
