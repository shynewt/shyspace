import { expect, type Locator, type Page, test } from "@playwright/test";

function thread(page: Page): Locator {
  return page.getByRole("complementary", { name: "Thread" });
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
});
