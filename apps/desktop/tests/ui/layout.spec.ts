import { expect, type Locator, type Page, test } from "@playwright/test";

/** Full screen down to a split screen's half, and the smallest supported window. */
const SIZES = [
  { width: 640, height: 560 },
  { width: 800, height: 800 },
  { width: 1024, height: 800 },
  { width: 1280, height: 800 },
  { width: 1600, height: 900 },
];
const LONG_CHANNEL = "customer-feedback-and-support-escalations";
const LONG_PAGE = "Quarterly planning notes for the customer feedback programme";
const LONG_URL = `https://example.com/${"a-very-long-path-segment-".repeat(12)}end`;
const LONG_CODE = `\`\`\`\nconst line = "${"x".repeat(400)}";\n\`\`\``;
const MIN_TITLE_PX = 120;

type ViewName = "channel" | "thread" | "inbox" | "later" | "drafts" | "page";

/**
 * Opens the app at `view` in a window of `size`: a private channel with a
 * long name, an agent, long links and code, a thread, a nested page with a
 * long title, a mention in the inbox, a saved message, a draft and a
 * message of yours.
 */
async function openAt(page: Page, size: { width: number; height: number }, view: ViewName) {
  await page.setViewportSize(size);
  await page.addInitScript(
    ({ view, names }) => {
      window.prepareFake = (fake) => {
        const grace = fake.addMember("Grace");
        fake.addMember("Helper", { kind: "bot" });
        const channel = fake.receivePrivateChannel(grace, names.channel);
        fake.receiveMessage(grace, names.channel, `Have a look ${names.url}`);
        fake.receiveMessage(grace, names.channel, names.code);
        const root = fake.receiveMessage(grace, names.channel, "Who owns the escalation rota?");
        fake.receiveReply(grace, root, `Me, see ${names.url}`);
        fake.receiveMessage(grace, "general", `Ping [@Ada](shy://member/${fake.session?.me ?? ""})`);
        fake.handle("save_for_later", { item: root, remindMs: null });
        fake.handle("set_draft", { target: root, text: `Draft with a long link ${names.url}` });
        fake.handle("send_message", { channel, text: `Sent with a long link ${names.url}`, replyTo: null });
        fake.seedPage("Planning", "Parent page");
        const pageId = fake.seedPage(names.page, "Some text", "Planning");
        const target =
          view === "channel"
            ? { kind: "channel", id: channel }
            : view === "thread"
              ? { kind: "channel", id: channel, thread: root }
              : view === "page"
                ? { kind: "page", id: pageId }
                : { kind: view };
        localStorage.setItem("shyspace.lastView", JSON.stringify({ me: fake.session?.me, view: target, anchor: null }));
      };
    },
    { view, names: { channel: LONG_CHANNEL, page: LONG_PAGE, url: LONG_URL, code: LONG_CODE } },
  );
  await page.goto("?seed=layout");
  await expect(viewHeader(page)).toBeVisible();
  if (view === "thread") await expect(thread(page).getByText("Me, see")).toBeVisible();
  await waitUntilStill(viewHeader(page));
  if (view === "thread") await waitUntilStill(thread(page));
}

/** Waits out entry animations (the shell scales in, views and panels slide), so boxes are final. */
async function waitUntilStill(locator: Locator) {
  await expect
    .poll(() =>
      locator.evaluate(async (element) => {
        const before = JSON.stringify(element.getBoundingClientRect());
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        return before === JSON.stringify(element.getBoundingClientRect());
      }),
    )
    .toBe(true);
}

function viewHeader(page: Page): Locator {
  return page.getByRole("main").locator("header").first();
}

function thread(page: Page): Locator {
  return page.getByRole("complementary", { name: "Thread" });
}

/** Elements that scroll sideways (or spill out of the window) though they shouldn't. */
async function expectNoSidewaysScroll(page: Page) {
  const offenders = await page.evaluate(() => {
    const found: string[] = [];
    const root = document.documentElement;
    if (root.scrollWidth > root.clientWidth) found.push(`the page (${root.scrollWidth} > ${root.clientWidth})`);
    for (const element of document.querySelectorAll<HTMLElement>("body *")) {
      const { overflowX } = getComputedStyle(element);
      if (overflowX !== "auto" && overflowX !== "scroll") continue;
      // Code and tables scroll inside their own box on purpose.
      if (element.matches("pre, .table-scroll, .cm-scroller")) continue;
      if (element.scrollWidth > element.clientWidth + 1) {
        found.push(
          `${element.tagName.toLowerCase()}.${element.className} (${element.scrollWidth} > ${element.clientWidth})`,
        );
      }
    }
    return found;
  });
  expect(offenders).toEqual([]);
}

/** One line high, with nothing spilling out or cut off at its ends. */
async function expectOneLineHeader(header: Locator) {
  const layout = await header.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const spilled = [...element.querySelectorAll<HTMLElement>("*")]
      .filter((child) => child.getClientRects().length > 0)
      .filter((child) => {
        const rect = child.getBoundingClientRect();
        return rect.width > 0 && (rect.left < box.left - 1 || rect.right > box.right + 1 || rect.height > box.height);
      })
      .map((child) => child.outerHTML.slice(0, 80));
    return { height: box.height, scrollHeight: element.scrollHeight, clientHeight: element.clientHeight, spilled };
  });
  expect(layout.height).toBeLessThanOrEqual(53);
  expect(layout.scrollHeight).toBeLessThanOrEqual(layout.clientHeight);
  expect(layout.spilled).toEqual([]);
}

/** On screen, not covered by anything, and at least 120 px wide unless its whole text is narrower. */
async function expectTitleWins(title: Locator) {
  await expect(title).toBeVisible();
  const layout = await title.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const probeX = box.left + Math.min(box.width, 120) / 2;
    const hit = document.elementFromPoint(probeX, box.top + box.height / 2);
    return {
      width: box.width,
      wholeWidth: element.scrollWidth,
      lines: Math.round(box.height / Number.parseFloat(getComputedStyle(element).lineHeight || "20")),
      isUncovered: hit !== null && (hit === element || element.contains(hit)),
    };
  });
  expect(layout.isUncovered).toBe(true);
  expect(layout.width).toBeGreaterThanOrEqual(Math.min(MIN_TITLE_PX, layout.wholeWidth) - 1);
}

/** A button anyone can see and click: on screen and not under something else. */
async function isClickable(button: Locator): Promise<boolean> {
  if ((await button.count()) === 0 || !(await button.first().isVisible())) return false;
  return button.first().evaluate((element) => {
    const box = element.getBoundingClientRect();
    if (box.right > window.innerWidth || box.bottom > window.innerHeight || box.left < 0) return false;
    const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    return hit !== null && (hit === element || element.contains(hit));
  });
}

/** Each action is a button in the header, or inside its Info popover. */
async function expectReachable(page: Page, header: Locator, actions: (string | RegExp)[]) {
  const info = header.getByRole("button", { name: "Channel info" });
  for (const action of actions) {
    if (await isClickable(header.getByRole("button", { name: action }))) continue;
    expect(await isClickable(info), `${action} is neither in the header nor behind Channel info`).toBe(true);
    await info.click();
    await expect(page.getByRole("dialog").getByText(action)).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toBeHidden();
  }
}

for (const size of SIZES) {
  test.describe(`at ${size.width}×${size.height}`, () => {
    test("a channel fits: one-line header, the title wins, every action reachable", async ({ page }) => {
      await openAt(page, size, "channel");
      const header = viewHeader(page);
      await expectNoSidewaysScroll(page);
      await expectOneLineHeader(header);
      await expectTitleWins(header.getByRole("heading", { name: LONG_CHANNEL }));
      await expectReachable(page, header, [
        `People in #${LONG_CHANNEL}`,
        /1 agent can read this/,
        "End-to-end encrypted",
      ]);
      // Long links and code stay inside the message list.
      const log = page.getByRole("log");
      await expect(log.getByText("Have a look")).toBeVisible();
      await expectNoSidewaysScroll(page);
    });

    test("a thread fits beside or over its channel", async ({ page }) => {
      await openAt(page, size, "thread");
      await expectNoSidewaysScroll(page);
      await expectOneLineHeader(viewHeader(page));
      await expectOneLineHeader(thread(page).locator("header"));
      await expectTitleWins(viewHeader(page).getByRole("heading", { name: LONG_CHANNEL }));
      expect(await isClickable(thread(page).getByRole("button", { name: "Close thread" }))).toBe(true);
      expect(await isClickable(thread(page).getByRole("button", { name: /Follow/ }))).toBe(true);
      const threadBox = await thread(page).boundingBox();
      expect(threadBox?.x ?? -1).toBeGreaterThanOrEqual(0);
      expect((threadBox?.x ?? 0) + (threadBox?.width ?? 0)).toBeLessThanOrEqual(size.width + 1);
    });

    test("the inbox fits", async ({ page }) => {
      await openAt(page, size, "inbox");
      const header = viewHeader(page);
      await expectNoSidewaysScroll(page);
      await expectOneLineHeader(header);
      await expectTitleWins(header.getByRole("heading", { name: "Inbox" }));
      await expectReachable(page, header, ["Mark all as read"]);
      const row = page.getByRole("button", { name: /mentioned you in/ });
      await expect(row).toBeVisible();
      expect(await row.evaluate((element) => element.getBoundingClientRect().height)).toBeLessThanOrEqual(72);
    });

    test("Later fits", async ({ page }) => {
      await openAt(page, size, "later");
      const header = viewHeader(page);
      await expectNoSidewaysScroll(page);
      await expectOneLineHeader(header);
      await expectTitleWins(header.getByRole("heading", { name: "Later" }));
      await expect(page.getByRole("list", { name: "Saved for later" }).getByRole("listitem")).toHaveCount(1);
    });

    test("Drafts & sent fits, in both tabs", async ({ page }) => {
      await openAt(page, size, "drafts");
      const header = viewHeader(page);
      await expectNoSidewaysScroll(page);
      await expectOneLineHeader(header);
      await expectTitleWins(header.getByRole("heading", { name: "Drafts & sent" }));
      const draft = page.getByRole("list", { name: "Drafts" }).getByRole("button", { name: /Draft with a long link/ });
      await expect(draft).toBeVisible();
      expect(await draft.evaluate((element) => element.getBoundingClientRect().height)).toBeLessThanOrEqual(72);

      await page.getByRole("tab", { name: "Sent" }).click();
      await expect(page.getByRole("tabpanel").getByRole("button", { name: /Sent with a long link/ })).toBeVisible();
      await expectNoSidewaysScroll(page);
    });

    test("a page fits, its title winning over the breadcrumb", async ({ page }) => {
      await openAt(page, size, "page");
      const header = viewHeader(page);
      await expectNoSidewaysScroll(page);
      await expectOneLineHeader(header);
      await expectTitleWins(header.locator('[aria-current="page"]'));
      await expectReachable(page, header, ["Copy link", "Save for later", "More page actions"]);
      const article = page.getByRole("main").locator("article");
      const articleBox = await article.boundingBox();
      expect(articleBox?.width ?? 0).toBeLessThanOrEqual(760);
    });

    test("settings fit, every section reachable and readable", async ({ page }) => {
      await openAt(page, size, "channel");
      await page.keyboard.press("Control+,");
      const settings = page.getByRole("dialog", { name: "Settings" });
      await waitUntilStill(settings);
      await expect(settings).toBeInViewport({ ratio: 1 });
      await expectNoSidewaysScroll(page);
      const tabs = settings.getByRole("tablist", { name: "Settings sections" }).getByRole("tab");
      for (const tab of await tabs.all()) {
        await tab.scrollIntoViewIfNeeded();
        await expect(tab).toBeInViewport({ ratio: 1 });
        const isLabelCut = await tab.evaluate((element) =>
          [...element.querySelectorAll("span")].some((span) => span.scrollWidth > span.clientWidth + 1),
        );
        expect(isLabelCut, `${await tab.textContent()} is cut off`).toBe(false);
        await tab.click();
        // Members has tabs of its own; the section's panel comes first.
        const panel = settings.getByRole("tabpanel").first();
        await expect(panel).toBeVisible();
        expect((await panel.boundingBox())?.width ?? 0).toBeGreaterThanOrEqual(380);
        await expectNoSidewaysScroll(page);
      }
    });
  });
}

test("a message's toolbar stays inside the message list in the smallest window", async ({ page }) => {
  await openAt(page, { width: 640, height: 560 }, "channel");
  const message = page.getByRole("article").filter({ hasText: "Who owns the escalation rota?" });
  await message.hover();
  const toolbar = message.getByRole("button", { name: "React" }).locator("xpath=..");
  await expect(toolbar).toHaveCSS("opacity", "1");
  const [toolbarBox, logBox] = await Promise.all([toolbar.boundingBox(), page.getByRole("log").boundingBox()]);
  if (!toolbarBox || !logBox) throw new Error("not laid out");
  expect(toolbarBox.x).toBeGreaterThanOrEqual(logBox.x);
  expect(toolbarBox.x + toolbarBox.width).toBeLessThanOrEqual(logBox.x + logBox.width);
  await expectNoSidewaysScroll(page);
});

test("the search palette and the emoji picker fit the smallest window", async ({ page }) => {
  await openAt(page, { width: 640, height: 560 }, "channel");
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Search" });
  await expect(palette).toBeInViewport({ ratio: 1 });
  await page.keyboard.press("Escape");
  await expect(palette).toBeHidden();

  await page.getByRole("button", { name: "Insert emoji" }).click();
  const picker = page.getByRole("dialog").filter({ has: page.getByPlaceholder("Search emoji") });
  await expect(picker).toBeVisible();
  await expect(picker).toBeInViewport({ ratio: 1 });
});

test("nothing breaks below the smallest supported window", async ({ page }) => {
  await openAt(page, { width: 420, height: 480 }, "channel");
  const header = viewHeader(page);
  await expectOneLineHeader(header);
  await expectTitleWins(header.getByRole("heading", { name: LONG_CHANNEL }));
  await expectReachable(page, header, [`People in #${LONG_CHANNEL}`, /1 agent can read this/, "End-to-end encrypted"]);
  await expectNoSidewaysScroll(page);
});
