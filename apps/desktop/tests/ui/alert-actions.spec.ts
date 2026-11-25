import { expect, type Page, test } from "@playwright/test";

/** Grace writes in #random (made for the test) while you're in #general; returns the message's id. */
async function graceWritesInRandom(page: Page, text: string): Promise<string> {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  return page.evaluate((text) => {
    window.fake.handle("create_channel", { name: "random" });
    return window.fake.receiveMessage(window.fake.addMember("Grace"), "random", text);
  }, text);
}

async function replyFromAlert(page: Page, message: string, text: string) {
  await page.evaluate(
    ([message, text]) =>
      window.fake.notifications.respond({
        action: "reply",
        workspace_id: window.fake.session?.workspace_id ?? "",
        message,
        text,
      }),
    [message, text],
  );
}

async function latestIn(page: Page, channelName: string) {
  return page.evaluate((channelName) => {
    const channel = window.fake.channelId(channelName);
    const messages = window.fake.handle("messages", { channel, before: null, limit: 1 }) as { text: string }[];
    return messages[0]?.text;
  }, channelName);
}

test("a reply from a notification goes into the channel, with mentions as the composer writes them", async ({
  page,
}) => {
  const message = await graceWritesInRandom(page, "Lunch?");

  await replyFromAlert(page, message, "@grace yes, and @here too");

  await expect
    .poll(() => latestIn(page, "random"))
    .toMatch(/^\[@Grace\]\(shy:\/\/.+\) yes, and \[@here\]\(shy:\/\/.+\) too$/);
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a reply from a notification to a thread reply stays in that thread", async ({ page }) => {
  const root = await graceWritesInRandom(page, "Plan for Friday");
  const reply = await page.evaluate(
    (root) => window.fake.receiveReply(window.fake.memberNamed("Grace"), root, "Any thoughts?"),
    root,
  );

  await replyFromAlert(page, reply, "Looks good");

  await expect
    .poll(() =>
      page.evaluate((root) => (window.fake.handle("thread", { root }) as { text: string }[]).at(-1)?.text, root),
    )
    .toBe("Looks good");
  expect(await latestIn(page, "random")).toBe("Plan for Friday");
});

test("a reply that can't be sent is kept as a draft and a notification says so", async ({ page }) => {
  const message = await graceWritesInRandom(page, "Lunch?");
  await page.evaluate(() => {
    window.fake.notifications.failReplies = true;
  });

  await replyFromAlert(page, message, "yes");

  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown))
    .toContainEqual({ title: "Your reply wasn't sent", body: "It's saved as a draft." });
  await page.getByRole("button", { name: /^random/ }).click();
  await expect(page.getByLabel("Message #random")).toHaveValue("yes");
});

test("a lost reply is said even while notifications are paused", async ({ page }) => {
  await graceWritesInRandom(page, "Lunch?");
  await page.evaluate(() => window.fake.handle("set_notifications_paused", { untilMs: Date.now() + 3_600_000 }));

  await replyFromAlert(page, "a-deleted-message", "yes");

  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown))
    .toContainEqual({ title: "Your reply wasn't sent", body: "It couldn't be kept as a draft either. You wrote: yes" });
});

test("a reply that waited too long is kept as a draft, not sent", async ({ page }) => {
  const message = await graceWritesInRandom(page, "Lunch?");

  await page.evaluate(
    (message) =>
      window.fake.notifications.respond({
        action: "keep_as_draft",
        workspace_id: window.fake.session?.workspace_id ?? "",
        message,
        text: "yes",
      }),
    message,
  );

  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown))
    .toContainEqual({ title: "Your reply wasn't sent", body: "It's saved as a draft." });
  expect(await latestIn(page, "random")).toBe("Lunch?");
  await page.getByRole("button", { name: /^random/ }).click();
  await expect(page.getByLabel("Message #random")).toHaveValue("yes");
});

test("mark as read from a notification reads the channel up to that message", async ({ page }) => {
  const message = await graceWritesInRandom(page, "one");
  await page.evaluate(() => window.fake.receiveMessage(window.fake.memberNamed("Grace"), "random", "two"));
  await expect(page.getByRole("button", { name: "random, 2 unread" })).toBeVisible();

  await page.evaluate(
    (message) =>
      window.fake.notifications.respond({
        action: "mark_read",
        workspace_id: window.fake.session?.workspace_id ?? "",
        target: { kind: "message", id: message },
      }),
    message,
  );

  // "two" came after the alert, so it stays unread.
  await expect(page.getByRole("button", { name: "random, 1 unread" })).toBeVisible();
});

test("a reply to a message that's gone says so, with what you wrote", async ({ page }) => {
  await graceWritesInRandom(page, "Lunch?");

  await replyFromAlert(page, "a-deleted-message", "yes");

  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown))
    .toContainEqual({ title: "Your reply wasn't sent", body: "It couldn't be kept as a draft either. You wrote: yes" });
});

/** Opens #random, so its message box is on screen while a reply is kept. */
async function openRandom(page: Page) {
  await page.getByRole("button", { name: /^random/ }).click();
  await expect(page.getByLabel("Message #random")).toBeVisible();
}

test("a reply kept as a draft shows in the message box already open there", async ({ page }) => {
  const message = await graceWritesInRandom(page, "Lunch?");
  await openRandom(page);
  await page.evaluate(() => {
    window.fake.notifications.failReplies = true;
  });

  await replyFromAlert(page, message, "yes");

  await expect(page.getByLabel("Message #random")).toHaveValue("yes");
});

test("a reply kept as a draft joins what you're typing there, losing neither", async ({ page }) => {
  const message = await graceWritesInRandom(page, "Lunch?");
  await openRandom(page);
  await page.evaluate(() => {
    window.fake.notifications.failReplies = true;
  });
  await page.getByLabel("Message #random").fill("half a thought");

  await replyFromAlert(page, message, "yes");

  await expect(page.getByLabel("Message #random")).toHaveValue("half a thought\n\nyes");
  await expect
    .poll(() =>
      page.evaluate(() => {
        const drafts = window.fake.handle("drafts", undefined) as { target: string; text: string }[];
        return drafts.find((draft) => draft.target === window.fake.channelId("random"))?.text;
      }),
    )
    .toBe("half a thought\n\nyes");
});

test("a reply from a notification leaves the draft open there as it was", async ({ page }) => {
  const message = await graceWritesInRandom(page, "Lunch?");
  await openRandom(page);
  await page.getByLabel("Message #random").fill("a longer answer, half written");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window.fake.handle("drafts", undefined) as { target: string; text: string }[]).find(
            (draft) => draft.target === window.fake.channelId("random"),
          )?.text,
      ),
    )
    .toBe("a longer answer, half written");

  await replyFromAlert(page, message, "yes");

  await expect.poll(() => latestIn(page, "random")).toBe("yes");
  await expect(page.getByLabel("Message #random")).toHaveValue("a longer answer, half written");
});
