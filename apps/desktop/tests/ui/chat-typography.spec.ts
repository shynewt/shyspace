import { expect, type Locator, type Page, test } from "@playwright/test";

/** Rich messages read like chat, not a document: compact headings, even spacing, nothing glued together. */

async function receive(page: Page, text: string): Promise<Locator> {
  await page.evaluate((text) => window.fake.receiveMessage(window.fake.addMember("Grace"), "general", text), text);
  const body = page.getByRole("article").last().locator(".prose-chat");
  await expect(body).toBeVisible();
  return body;
}

const box = async (element: Locator) => {
  const found = await element.boundingBox();
  if (!found) throw new Error("not laid out");
  return found;
};

const fontSize = (element: Locator) => element.evaluate((node) => Number.parseFloat(getComputedStyle(node).fontSize));

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("headings stand out from the text, smaller than a page's", async ({ page }) => {
  const body = await receive(page, "# One\n## Two\n### Three\nplain words");
  const [one, two, three, text] = await Promise.all([
    fontSize(body.getByRole("heading", { name: "One" })),
    fontSize(body.getByRole("heading", { name: "Two" })),
    fontSize(body.getByRole("heading", { name: "Three" })),
    fontSize(body.getByText("plain words")),
  ]);
  expect(one).toBeGreaterThan(two);
  expect(two).toBeGreaterThan(three);
  expect(three).toBeGreaterThan(text);
  expect(one).toBeLessThanOrEqual(text * 1.5);
  const weight = await body
    .getByRole("heading", { name: "Three" })
    .evaluate((node) => Number(getComputedStyle(node).fontWeight));
  expect(weight).toBeGreaterThanOrEqual(600);
});

test("a divider has even room above and below, across the message", async ({ page }) => {
  const body = await receive(page, "- last item\n\n---\n\nnext part");
  const item = await box(body.getByText("last item"));
  const rule = await box(body.locator("hr"));
  const next = await box(body.getByText("next part"));
  const above = rule.y - (item.y + item.height);
  const below = next.y - (rule.y + rule.height);
  expect(above).toBeGreaterThanOrEqual(8);
  expect(Math.abs(above - below)).toBeLessThanOrEqual(1);
  expect(Math.abs(rule.width - (await box(body)).width)).toBeLessThanOrEqual(1);
  const color = await body.locator("hr").evaluate((node) => getComputedStyle(node).borderTopColor);
  const line = await page.evaluate(() => {
    const probe = document.createElement("div");
    probe.style.color = "var(--border)";
    document.body.append(probe);
    const value = getComputedStyle(probe).color;
    probe.remove();
    return value;
  });
  expect(color).toBe(line);
});

test("blocks are spaced evenly, with none before the first", async ({ page }) => {
  const body = await receive(page, "one\n\n- two\n\n> three\n\n```\nfour\n```\n\n| five |\n| --- |\n| six |");
  const blocks = body.locator(":scope > *");
  const boxes = await Promise.all((await blocks.all()).map(box));
  const gaps = boxes.slice(1).map((current, index) => current.y - (boxes[index].y + boxes[index].height));
  expect(gaps.length).toBe(4);
  for (const gap of gaps) expect(Math.abs(gap - gaps[0])).toBeLessThanOrEqual(1);
  expect(gaps[0]).toBeGreaterThan(2);
  expect(boxes[0].y - (await box(body)).y).toBeLessThanOrEqual(0.5);
});

test("a nested list sits as close as the items around it", async ({ page }) => {
  const body = await receive(page, "- first\n- second\n  - inner\n- third");
  // Each item's own line of text, without the list nested in it.
  const lines = await body.locator("li").evaluateAll((items) =>
    items.map((item) => {
      const range = document.createRange();
      range.selectNodeContents(item.firstChild ?? item);
      const { y, height } = range.getBoundingClientRect();
      return { y, height };
    }),
  );
  const [first, second, inner] = lines;
  const itemGap = second.y - (first.y + first.height);
  const nestedGap = inner.y - (second.y + second.height);
  expect(Math.abs(nestedGap - itemGap)).toBeLessThanOrEqual(1);
});

test("code shows exactly the characters written", async ({ page }) => {
  const body = await receive(page, "`a => b` and\n```\nx => y != z\n```");
  for (const code of await body.locator("code").all()) {
    expect(await code.evaluate((node) => getComputedStyle(node).fontVariantLigatures)).toBe("none");
  }
});

test("a plain one-line message is one line tall, as before", async ({ page }) => {
  const body = await receive(page, "Morning! Just a line.");
  const lineHeight = await body.evaluate((node) => Number.parseFloat(getComputedStyle(node).lineHeight));
  expect(Math.abs((await box(body)).height - lineHeight)).toBeLessThanOrEqual(1);
});
