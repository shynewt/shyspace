import { expect, type Page, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
  await page.evaluate(() => window.fake.addMember("Grace"));
});

async function openThreadOf(page: Page, text: string) {
  await page.getByRole("log", { name: "Messages in #general" }).getByRole("article").filter({ hasText: text }).hover();
  await page.getByRole("button", { name: "Reply in thread" }).click();
  await expect(page.getByRole("complementary", { name: "Thread" })).toBeVisible();
}

test("replies go into a thread beside the channel", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Release today?");
  await composer.press("Enter");

  await openThreadOf(page, "Release today?");
  const thread = page.getByRole("complementary", { name: "Thread" });
  await thread.getByLabel("Reply in thread").fill("After lunch");
  await thread.getByLabel("Reply in thread").press("Enter");

  await expect(thread.getByRole("article").filter({ hasText: "After lunch" })).toBeVisible();
  const channel = page.getByRole("log", { name: "Messages in #general" });
  await expect(channel.getByRole("button", { name: /1 reply/ })).toBeVisible();
  // The reply has no message of its own in the channel; only the summary under its first message quotes it.
  await expect(
    channel.getByRole("article").filter({ hasText: "After lunch" }).filter({ hasNotText: "Release today?" }),
  ).toHaveCount(0);
});

test("new replies in a thread you follow show until you open it", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Who's on call?");
  await composer.press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "Who's on call?" })).toBeVisible();

  await page.evaluate(() =>
    window.fake.receiveReply(window.fake.memberNamed("Grace"), window.fake.messageWithText("Who's on call?"), "Me"),
  );
  const summary = page.getByRole("button", { name: /1 reply.*1 new/ });
  await expect(summary).toBeVisible();

  await summary.click();
  await expect(
    page
      .getByRole("complementary", { name: "Thread" })
      .getByRole("article")
      .filter({ has: page.getByText("Me", { exact: true }) }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: /1 new/ })).toHaveCount(0);
});

test("you can stop and start following a thread", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Plans for Friday");
  await composer.press("Enter");
  await openThreadOf(page, "Plans for Friday");

  const thread = page.getByRole("complementary", { name: "Thread" });
  await thread.getByRole("button", { name: "Following" }).click();
  await expect(thread.getByRole("button", { name: "Follow", exact: true })).toBeVisible();
  await thread.getByRole("button", { name: "Follow", exact: true }).click();
  await expect(thread.getByRole("button", { name: "Following" })).toBeVisible();

  await thread.getByRole("button", { name: "Close thread" }).click();
  await expect(thread).toHaveCount(0);
});

test("the thread keeps up with new replies while you're at the end", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Release today?");
  await composer.press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "Release today?" })).toBeVisible();
  await page.evaluate(() => {
    const root = window.fake.messageWithText("Release today?");
    for (let index = 1; index <= 25; index += 1) {
      window.fake.receiveReply(window.fake.memberNamed("Grace"), root, `Reply number ${index}`);
    }
  });
  await openThreadOf(page, "Release today?");
  const thread = page.getByRole("complementary", { name: "Thread" });
  await expect(thread.getByRole("article").filter({ hasText: "Reply number 25" })).toBeInViewport();

  await page.evaluate(() =>
    window.fake.receiveReply(
      window.fake.memberNamed("Grace"),
      window.fake.messageWithText("Release today?"),
      "Fresh one",
    ),
  );
  await expect(thread.getByRole("article").filter({ hasText: "Fresh one" })).toBeInViewport();

  await thread.getByLabel("Reply in thread").fill("Mine too");
  await thread.getByLabel("Reply in thread").press("Enter");
  await expect(thread.getByRole("article").filter({ hasText: "Mine too" })).toBeInViewport();
});

test("a thread opened at a reply still follows new replies", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.memberNamed("Grace");
    const question = window.fake.receiveMessage(grace, "general", "Which version ships?");
    window.fake.receiveReply(grace, question, "Version two ships Friday");
  });
  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("Friday");
  await page.getByRole("option", { name: /Version two/ }).click();
  const thread = page.getByRole("complementary", { name: "Thread" });
  await expect(thread.getByRole("article").filter({ hasText: "Version two ships Friday" })).toBeVisible();

  await page.evaluate(() => {
    const root = window.fake.messageWithText("Which version ships?");
    for (let index = 1; index <= 25; index += 1) {
      window.fake.receiveReply(window.fake.memberNamed("Grace"), root, `Follow-up ${index}`);
    }
  });
  await expect(thread.getByRole("article").filter({ hasText: "Follow-up 25" })).toBeInViewport();
});

async function postWithReplies(page: Page, text: string, replies: [string, string][]) {
  const composer = page.getByLabel("Message #general");
  await composer.fill(text);
  await composer.press("Enter");
  await expect(page.getByRole("article").filter({ hasText: text })).toBeVisible();
  await page.evaluate(
    ({ root, replies }) => {
      const authors = new Map([["Grace", window.fake.memberNamed("Grace")]]);
      for (const [name, reply] of replies) {
        const author = authors.get(name) ?? window.fake.addMember(name);
        authors.set(name, author);
        const text = reply.replaceAll("shy://member/ME", `shy://member/${window.fake.session?.me}`);
        window.fake.receiveReply(author, window.fake.messageWithText(root), text);
      }
    },
    { root: text, replies },
  );
}

function summaryOf(page: Page, text: string) {
  return page
    .getByRole("log", { name: "Messages in #general" })
    .getByRole("article")
    .filter({ hasText: text })
    .getByRole("button", { name: /Open thread$/ });
}

test("the summary shows the latest repliers, the count, and the last reply", async ({ page }) => {
  await postWithReplies(page, "Plans for Friday?", [
    ["Kai", "I can"],
    ["Lee", "maybe"],
    ["Kai", "or Saturday"],
    ["Grace", "Friday works, I'll tell the **team**\n\nand ping [@Ada](shy://member/ME)"],
  ]);
  const summary = summaryOf(page, "Plans for Friday?");
  await summary.click();
  await page.getByRole("complementary", { name: "Thread" }).getByRole("button", { name: "Close thread" }).click();

  await expect(summary).toHaveAccessibleName(/^4 replies, last from Grace at .+\. Open thread$/);
  await expect(summary.locator("span.relative")).toHaveText(["G", "K", "L"]);
  await expect(summary).toContainText("4 replies");
  await expect(summary).toContainText("Grace: Friday works, I'll tell the team and ping @Ada");
  await expect(summary.locator("time")).toBeVisible();
  await expect(summary.getByText(/new$/)).toHaveCount(0);
});

test("unread replies make the count bold and say how many are new", async ({ page }) => {
  await postWithReplies(page, "Who's on call?", [
    ["Kai", "not me"],
    ["Grace", "me"],
  ]);
  const summary = summaryOf(page, "Who's on call?");
  await expect(summary).toHaveAccessibleName(/^2 replies, last from Grace at .+, 2 new\. Open thread$/);
  await expect(summary.getByText("2 new", { exact: true })).toBeVisible();
  await expect(summary.getByText("2 replies", { exact: true })).toHaveCSS("font-weight", "600");
});

test("a long last reply stays on one line and the count never wraps", async ({ page }) => {
  await page.setViewportSize({ width: 720, height: 800 });
  await postWithReplies(page, "Long one", [["Grace", "word ".repeat(120)]]);
  const summary = summaryOf(page, "Long one");
  await expect(summary).toBeVisible();

  const layout = await summary.evaluate((button) => {
    const preview = [...button.querySelectorAll("span")].find((span) => span.textContent?.startsWith("Grace:"));
    const count = [...button.querySelectorAll("span")].find((span) => span.textContent === "1 reply");
    const column = button.parentElement?.getBoundingClientRect();
    if (!preview || !count || !column) throw new Error("summary parts missing");
    return {
      height: button.getBoundingClientRect().height,
      isPreviewCut: preview.scrollWidth > preview.clientWidth,
      countHeight: count.getBoundingClientRect().height,
      isCountWhole: count.scrollWidth <= count.clientWidth,
      overflow: button.getBoundingClientRect().right - column.right,
    };
  });
  expect(layout.height).toBeLessThanOrEqual(28);
  expect(layout.isPreviewCut).toBe(true);
  expect(layout.countHeight).toBeLessThanOrEqual(20);
  expect(layout.isCountWhole).toBe(true);
  expect(layout.overflow).toBeLessThanOrEqual(0);
});

test("Enter on the summary opens the thread", async ({ page }) => {
  await postWithReplies(page, "Keyboard question", [["Grace", "keyboard answer"]]);
  const summary = summaryOf(page, "Keyboard question");
  await summary.focus();
  await page.keyboard.press("Enter");

  await expect(
    page.getByRole("complementary", { name: "Thread" }).getByRole("article").filter({ hasText: "keyboard answer" }),
  ).toBeVisible();
});
