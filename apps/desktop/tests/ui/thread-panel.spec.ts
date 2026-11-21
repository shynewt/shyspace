import { expect, type Locator, type Page, test } from "@playwright/test";

function thread(page: Page): Locator {
  return page.getByRole("complementary", { name: "Thread" });
}

/** A message's text runs under the thread, so click its start, which stays uncovered. */
function clickChannelMessage(page: Page) {
  return page
    .getByRole("log")
    .getByText("Another topic entirely")
    .click({ position: { x: 8, y: 6 } });
}

async function rightEdge(locator: Locator): Promise<number | undefined> {
  const box = await locator.boundingBox();
  return box ? box.x + box.width : undefined;
}

test.describe("the thread panel", () => {
  async function openThread(page: Page, size: { width: number; height: number }) {
    await page.setViewportSize(size);
    await page.addInitScript(() => {
      window.prepareFake = (fake) => {
        const grace = fake.addMember("Grace");
        const root = fake.receiveMessage(grace, "general", "Plans for the offsite");
        fake.receiveReply(grace, root, "Lisbon?");
        fake.receiveMessage(grace, "general", "Another topic entirely");
      };
    });
    await page.goto("");
    const summary = page.getByRole("log").getByRole("button", { name: /1 reply/ });
    await summary.click();
    await expect(thread(page).getByText("Lisbon?")).toBeVisible();
    return summary;
  }

  test("docks beside the channel in a wide window", async ({ page }) => {
    await openThread(page, { width: 1600, height: 900 });
    await expect.poll(() => rightEdge(thread(page))).toBe(1600);
    const threadBox = await thread(page).boundingBox();
    const logBox = await page.getByRole("log").boundingBox();
    if (!threadBox || !logBox) throw new Error("not laid out");
    expect(threadBox.width).toBe(380);
    expect(logBox.x + logBox.width).toBeLessThanOrEqual(threadBox.x + 1);
    expect(logBox.width).toBeGreaterThanOrEqual(560);
  });

  test("slides over the channel when there's no room beside it", async ({ page }) => {
    const summary = await openThread(page, { width: 1024, height: 800 });
    await expect.poll(() => rightEdge(thread(page))).toBe(1024);
    const threadBox = await thread(page).boundingBox();
    const logBox = await page.getByRole("log").boundingBox();
    if (!threadBox || !logBox) throw new Error("not laid out");
    expect(logBox.x + logBox.width).toBeGreaterThan(threadBox.x + 100);
    await expect(thread(page)).not.toHaveCSS("box-shadow", "none");
    await expect(thread(page).getByLabel("Reply in thread")).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(thread(page)).toBeHidden();
    await expect(summary).toBeFocused();

    await page.keyboard.press("Alt+ArrowLeft");
    await expect(thread(page).getByText("Lisbon?")).toBeVisible();
    await page.keyboard.press("Alt+ArrowRight");
    await expect(thread(page)).toBeHidden();
  });

  test("a narrow message box hides its key hints", async ({ page }) => {
    await openThread(page, { width: 1600, height: 900 });
    await expect(thread(page).getByLabel("Reply in thread")).toBeFocused();
    await expect(thread(page).getByText("new line")).toBeHidden();
    await page.getByLabel("Message #general").focus();
    await expect(page.getByText("new line").filter({ visible: true })).toHaveCount(1);
    await expect(thread(page).getByText("new line")).toBeHidden();
  });

  test("Esc over the thread closes only the thread, not the channel's unread line", async ({ page }) => {
    await openThread(page, { width: 1024, height: 800 });
    const line = page.getByRole("separator", { name: "New messages" });
    await expect(line).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(thread(page)).toBeHidden();
    await expect(line).toBeVisible();
  });

  test.describe("over the channel", () => {
    const NARROW = { width: 1024, height: 800 };

    async function openOverChannel(page: Page) {
      const summary = await openThread(page, NARROW);
      await expect.poll(() => rightEdge(thread(page))).toBe(NARROW.width);
      return summary;
    }

    test("closes when you click the channel, and focus stays where you clicked", async ({ page }) => {
      await openOverChannel(page);
      await clickChannelMessage(page);
      await expect(thread(page)).toHaveCount(0);

      await page.keyboard.press("Alt+ArrowLeft");
      await expect(thread(page).getByText("Lisbon?")).toBeVisible();
      await page.getByLabel("Message #general").click();
      await expect(thread(page)).toHaveCount(0);
      await expect(page.getByLabel("Message #general")).toBeFocused();
    });

    test("stays open for clicks inside it", async ({ page }) => {
      await openOverChannel(page);
      await thread(page).getByText("Lisbon?").click();
      await thread(page).getByRole("heading", { name: "Thread" }).click();
      await expect(thread(page).getByText("Lisbon?")).toBeVisible();
    });

    test("stays open while you use its emoji picker and message menu", async ({ page }) => {
      await openOverChannel(page);
      await thread(page).getByRole("button", { name: "Insert emoji" }).click();
      await page.getByLabel("Search emoji").click();
      await page.getByLabel("Search emoji").fill("tada");
      await page.getByRole("gridcell", { name: "Party popper" }).click();
      await expect(thread(page).getByLabel("Reply in thread")).toHaveValue(/🎉/);

      const reply = thread(page).getByRole("article").filter({ hasText: "Lisbon?" });
      await reply.hover();
      await reply.getByRole("button", { name: "More actions" }).click();
      const menu = page.getByRole("menu");
      await expect(menu).toBeVisible();
      await menu.click({ position: { x: 4, y: 4 } });
      await page.keyboard.press("Escape");
      await expect(menu).toHaveCount(0);
      await expect(thread(page).getByText("Lisbon?")).toBeVisible();
    });

    test("keeps the reply you were writing", async ({ page }) => {
      const summary = await openOverChannel(page);
      await thread(page).getByLabel("Reply in thread").fill("Half a thought");
      await clickChannelMessage(page);
      await expect(thread(page)).toHaveCount(0);

      await summary.click();
      await expect(thread(page).getByLabel("Reply in thread")).toHaveValue("Half a thought");
    });

    test("clicking the line that opened it closes it rather than reopening it", async ({ page }) => {
      const summary = await openOverChannel(page);
      await summary.click();
      await expect(thread(page)).toHaveCount(0);
      // Nothing else reopens it a moment later.
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      await expect(thread(page)).toHaveCount(0);

      await summary.click();
      await expect(thread(page).getByText("Lisbon?")).toBeVisible();
    });

    test("back and forward restore it after a click outside closed it", async ({ page }) => {
      await openOverChannel(page);
      await clickChannelMessage(page);
      await expect(thread(page)).toHaveCount(0);
      await page.keyboard.press("Alt+ArrowLeft");
      await expect(thread(page).getByText("Lisbon?")).toBeVisible();
      await page.keyboard.press("Alt+ArrowRight");
      await expect(thread(page)).toHaveCount(0);
    });
  });

  test("docked beside the channel, it stays open for clicks on the channel", async ({ page }) => {
    await openThread(page, { width: 1600, height: 900 });
    await expect.poll(() => rightEdge(thread(page))).toBe(1600);
    await clickChannelMessage(page);
    await page.getByLabel("Message #general").click();
    await expect(thread(page).getByText("Lisbon?")).toBeVisible();
  });
});
