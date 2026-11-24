import { expect, test } from "@playwright/test";
import { allowClipboard } from "./support";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
});

test("sending a message shows it once and clears the composer", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Hello team");
  await composer.press("Enter");

  await expect(page.getByRole("article").getByText("Hello team")).toHaveCount(1);
  await expect(composer).toHaveValue("");
});

test("shift enter adds a line instead of sending", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("first line");
  await composer.press("Shift+Enter");
  await composer.pressSequentially("second line");

  await expect(composer).toHaveValue("first line\nsecond line");
  await expect(page.getByRole("article")).toHaveCount(0);
});

test("a new channel appears once and opens", async ({ page }) => {
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("design");
  await page.getByLabel("Channel name").press("Enter");

  await expect(page.getByRole("button", { name: "design", exact: true })).toHaveCount(1);
  await expect(page.getByLabel("Message #design")).toBeVisible();
});

test("messages from other members appear live", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", "Morning! **Bold** plan");
  });

  await expect(page.getByText("Grace")).toBeVisible();
  await expect(page.locator("strong", { hasText: "Bold" })).toBeVisible();
});

test("reacting adds a count that a second click removes", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", "Shipped it");
  });
  const message = page.getByText("Shipped it");
  await message.hover();
  await page.getByRole("button", { name: "React" }).click();
  await page.getByRole("button", { name: "React with 👍" }).click();

  const reaction = page.getByRole("button", { name: "👍 1" });
  await expect(reaction).toBeVisible();
  await reaction.click();
  await expect(reaction).toHaveCount(0);
});

test("untrusted markdown never becomes live HTML", async ({ page }) => {
  await page.evaluate(() => {
    const mallory = window.fake.addMember("Mallory");
    window.fake.receiveMessage(mallory, "general", '<img src=x onerror="window.pwned=1"> <b>not bold</b>');
  });

  await expect(page.getByText("Mallory")).toBeVisible();
  await expect(page.locator("main img")).toHaveCount(0);
  expect(await page.evaluate(() => "pwned" in window)).toBe(false);
});

test("the channel says when agents can read it", async ({ page }) => {
  const note = page.getByText(/agents? can read this/);
  await expect(note).toHaveCount(0);
  await page.evaluate(() => window.fake.addMember("Claude", { kind: "bot" }));
  await expect(page.getByText("1 agent can read this")).toBeVisible();
});

test("a private channel counts only the agents in it", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const claude = window.fake.addMember("Claude", { kind: "bot" });
    window.fake.receivePrivateChannel(grace, "ops");
    window.fake.receivePrivateChannel(grace, "ai-lab", [claude]);
  });
  const sidebar = page.getByRole("navigation", { name: "Workspace" });
  await sidebar.getByRole("button", { name: /^ops/ }).click();
  await expect(page.getByLabel("Message #ops")).toBeVisible();
  await expect(page.getByText(/agents? can read this/)).toHaveCount(0);

  await sidebar.getByRole("button", { name: /^ai-lab/ }).click();
  await expect(page.getByText("1 agent can read this")).toBeVisible();
});

async function send(page: import("@playwright/test").Page, ...texts: string[]) {
  const composer = page.getByLabel("Message #general");
  for (const text of texts) {
    await composer.fill(text);
    await composer.press("Enter");
    await expect(page.getByRole("article").filter({ hasText: text })).toBeVisible();
  }
}

test("a follow-up message stays one line tall", async ({ page }) => {
  await send(page, "First thought", "Second thought");

  const followUp = await page.getByRole("article").filter({ hasText: "Second thought" }).boundingBox();
  expect(followUp?.height).toBeLessThanOrEqual(28);
});

test("the edited mark sits on the message's last line", async ({ page }) => {
  await send(page, "Lunch at noon");
  const composer = page.getByLabel("Message #general");
  await composer.press("ArrowUp");
  await expect(composer).toHaveValue("Lunch at noon");
  await composer.fill("Lunch at one");
  await composer.press("Enter");

  const message = page.getByRole("article").filter({ hasText: "Lunch at one" });
  const text = await message.getByText("Lunch at one").boundingBox();
  const mark = await message.getByText("(edited)").boundingBox();
  if (!text || !mark) throw new Error("message not laid out");
  expect(mark.y).toBeLessThan(text.y + text.height);
});

test("a message's toolbar goes away once the pointer moves on, even after using it", async ({ page }) => {
  await send(page, "Alpha", "Beta");
  const alpha = page.getByRole("article").filter({ hasText: "Alpha" });
  await alpha.hover();
  await alpha.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Save for later" }).click();

  await page.getByRole("article").filter({ hasText: "Beta" }).hover();
  const toolbar = alpha.getByRole("button", { name: "React" }).locator("xpath=..");
  await expect(toolbar).toHaveCSS("opacity", "0");
});

test("the composer's key hints show only while it's focused and empty", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  const hints = page.getByText("new line");
  await composer.focus();
  await expect(hints).toHaveCount(1);
  const before = await composer.boundingBox();

  await composer.fill("Hello");
  await expect(hints).toHaveCount(0);
  expect(await composer.boundingBox()).toEqual(before);
  await composer.fill("");
  await expect(hints).toHaveCount(1);
  await page.getByRole("log", { name: "Messages in #general" }).click();
  await expect(hints).toHaveCount(0);
});

test("a tall code block folds until opened, and copies its code", async ({ page, context, browserName }) => {
  await allowClipboard(context, browserName);
  const code = Array.from({ length: 40 }, (_, index) => `line ${index + 1}`).join("\n");
  await page.getByLabel("Message #general").fill(`\`\`\`\n${code}\n\`\`\``);
  await page.getByLabel("Message #general").press("Enter");
  const message = page.getByRole("article").filter({ hasText: "line 1" });
  const block = message.locator("pre");
  expect((await block.boundingBox())?.height).toBeLessThanOrEqual(260);

  await message.getByRole("button", { name: "Show more" }).click();
  expect((await block.boundingBox())?.height).toBeGreaterThan(500);
  await expect(message.getByRole("button", { name: "Show less" })).toBeVisible();

  await block.hover();
  await message.getByRole("button", { name: "Copy code" }).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(code);
});

test("an opened code block stays open when a new message arrives", async ({ page }) => {
  const code = Array.from({ length: 40 }, (_, index) => `line ${index + 1}`).join("\n");
  await page.getByLabel("Message #general").fill(`\`\`\`\n${code}\n\`\`\``);
  await page.getByLabel("Message #general").press("Enter");
  const message = page.getByRole("article").filter({ hasText: "line 1" });
  await message.getByRole("button", { name: "Show more" }).click();

  await page.getByLabel("Message #general").fill("Next");
  await page.getByLabel("Message #general").press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "Next" })).toBeVisible();
  await expect(message.getByRole("button", { name: "Show less" })).toBeVisible();
});

test("a short code block doesn't fold", async ({ page }) => {
  await page.getByLabel("Message #general").fill("```\nshort\n```");
  await page.getByLabel("Message #general").press("Enter");
  const message = page.getByRole("article").filter({ hasText: "short" });
  await expect(message.locator("pre")).toBeVisible();
  await expect(message.getByRole("button", { name: "Show more" })).toHaveCount(0);
});

test("the composer's lines are as tall as a message's, and one line fits beside the buttons", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Same height");
  await composer.press("Enter");
  const message = page.getByRole("article").locator(".prose-chat");
  await expect(message).toContainText("Same height");

  const lineHeight = (element: HTMLElement) => getComputedStyle(element).lineHeight;
  expect(await composer.evaluate(lineHeight)).toBe(await message.evaluate(lineHeight));
  const composerBox = await composer.boundingBox();
  const sendBox = await page.getByRole("button", { name: "Send" }).boundingBox();
  expect(composerBox?.height).toBe(sendBox?.height);
});

test("tables and callouts show formatted", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("| Area | Who |\n| --- | --- |\n| Website | **Maya** |\n| Release | Theo |");
  await composer.press("Enter");
  const table = page.getByRole("article").getByRole("table");
  await expect(table.getByRole("columnheader")).toHaveText(["Area", "Who"]);
  await expect(table.getByRole("row")).toHaveCount(3);
  await expect(table.getByRole("cell", { name: "Maya" }).locator("strong")).toHaveText("Maya");

  await composer.fill("> [!WARNING]\n> The bucket keys rotate on **Friday**.");
  await composer.press("Enter");
  const callout = page.getByRole("article").getByRole("note");
  await expect(callout).toHaveText(/^Warning\s*The bucket keys rotate on Friday\.\s*$/);
  await expect(callout).toHaveClass(/callout-warning/);

  await composer.fill("> [!TIP] Keep pull requests small.");
  await composer.press("Enter");
  await expect(page.getByRole("article").getByRole("note").filter({ hasText: "Tip" })).toHaveText(
    /^Tip\s*Keep pull requests small\.\s*$/,
  );

  await composer.fill("> [!FOO]\n> just a quote");
  await composer.press("Enter");
  await expect(page.getByRole("article").locator("blockquote")).toContainText("[!FOO]");
});

test("code in a named language is coloured, and unknown languages stay plain", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill('```ts\nconst answer = 42; // why\nlet name = "Ada";\n```');
  await composer.press("Enter");
  const code = page.getByRole("article").filter({ hasText: "const answer" }).locator("pre code");
  await expect(code.locator(".tok-keyword").first()).toHaveText("const");
  await expect(code.locator(".tok-number")).toHaveText("42");
  await expect(code.locator(".tok-comment")).toHaveText("// why");
  await expect(code.locator(".tok-string")).toHaveText('"Ada"');
  await expect(code).toHaveText('const answer = 42; // why\nlet name = "Ada";\n');

  await composer.fill("```nonsense\nconst stays plain\n```");
  await composer.press("Enter");
  const plain = page.getByRole("article").filter({ hasText: "stays plain" }).locator("pre code");
  await expect(plain).toHaveText("const stays plain\n");
  await expect(plain.locator("[class^=tok-]")).toHaveCount(0);
});

test("messages out of time order get one divider per change of day, and no stray ones", async ({ page }) => {
  const receive = (text: string) =>
    page.evaluate((body) => window.fake.receiveMessage(window.fake.memberNamed("Grace"), "general", body), text);
  await page.clock.install({ time: new Date("2026-11-22T09:00:00") });
  await page.goto("");
  await page.evaluate(() => window.fake.addMember("Grace"));
  await page.getByRole("button", { name: "general", exact: true }).click();
  await receive("first today");
  // From a device whose clock is a day behind.
  await page.clock.setSystemTime(new Date("2026-11-21T09:05:00"));
  await receive("from behind");
  await page.clock.setSystemTime(new Date("2026-11-22T09:10:00"));
  await receive("later today");

  await expect(page.getByRole("article").filter({ hasText: "later today" })).toBeVisible();
  const log = page.getByRole("log");
  await expect(log.getByText("Today", { exact: true })).toHaveCount(2);
  await expect(log.getByText("Yesterday", { exact: true })).toHaveCount(1);
});

test("a message shows twenty reactions at most, and the rest on request", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "So many feelings");
    for (let face = 0x1f600; face < 0x1f600 + 30; face++) {
      window.fake.receiveReaction(grace, message, String.fromCodePoint(face));
    }
  });
  const message = page.getByRole("article").filter({ hasText: "So many feelings" });
  const reactions = message.locator("button[aria-pressed]");
  await expect(reactions).toHaveCount(20);
  await message.getByRole("button", { name: "Show 10 more reactions" }).click();
  await expect(reactions).toHaveCount(30);
});
