import { expect, type Page, test } from "@playwright/test";

declare global {
  interface Window {
    openedUrls: string[];
  }
}

const lines = (page: Page) => page.locator(".cm-line");

async function openPage(page: Page, title: string) {
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: title, exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue(title);
}

async function seedAndOpen(page: Page, title: string, text: string) {
  await page.goto("");
  await page.evaluate(([title, text]) => window.fake.seedPage(title, text), [title, text]);
  await openPage(page, title);
}

/** The color of the text `wanted` in the editor. */
function colorOf(page: Page, wanted: string): Promise<string> {
  return page.locator(".cm-content").evaluate((root, wanted) => {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (node.textContent?.includes(wanted) && node.parentElement) return getComputedStyle(node.parentElement).color;
    }
    throw new Error(`no text "${wanted}" in the editor`);
  }, wanted);
}

test("markdown marks stay hidden until you edit their line", async ({ page }) => {
  await seedAndOpen(
    page,
    "Plan",
    "# Goals\nShip **fast** and *well* with `care`\n> Quote [the spec](https://example.com/spec)\n- first item\n1. step one\n---\nLast line",
  );

  await expect(lines(page)).toHaveText([
    "Goals",
    "Ship fast and well with care",
    "Quote the spec",
    "first item",
    "1. step one",
    "",
    "Last line",
  ]);

  await lines(page).nth(1).click();
  await expect(lines(page).nth(1)).toHaveText("Ship **fast** and *well* with `care`");
  await expect(lines(page).first()).toHaveText("Goals");

  await page.keyboard.press("ArrowUp");
  await expect(lines(page).first()).toHaveText("# Goals");
  await expect(lines(page).nth(1)).toHaveText("Ship fast and well with care");
});

test("list text has the body color; only the marker stands apart", async ({ page }) => {
  await seedAndOpen(page, "Lists", "Plain words\n\n- listed words\n\n7. numbered words");
  await expect(lines(page).nth(4)).toHaveText("7. numbered words");

  const body = await colorOf(page, "Plain words");
  expect(await colorOf(page, "listed words")).toBe(body);
  expect(await colorOf(page, "numbered words")).toBe(body);
  expect(await colorOf(page, "7.")).not.toBe(body);
});

test("checklist boxes toggle their item in the page", async ({ page }) => {
  await seedAndOpen(page, "Todo", "- [ ] Write the plan\n- [x] Book a room");

  const write = page.getByRole("checkbox", { name: "Write the plan" });
  await expect(write).not.toBeChecked();
  await expect(page.getByRole("checkbox", { name: "Book a room" })).toBeChecked();

  await write.click();

  await expect
    .poll(() => page.evaluate(() => window.fake.pageText("Todo")))
    .toBe("- [x] Write the plan\n- [x] Book a room");
  await expect(page.getByRole("checkbox", { name: "Write the plan" })).toBeChecked();
});

test("links show where they go and open with ctrl+click", async ({ page }) => {
  await seedAndOpen(page, "Links", "Read [the spec](https://example.com/spec) first\nsecond line");
  await page.evaluate(() => {
    window.openedUrls = [];
    const answer = window.fake.handle.bind(window.fake);
    window.fake.handle = (command, args) => {
      if (command !== "plugin:opener|open_url") return answer(command, args);
      if (args && "url" in args && typeof args.url === "string") window.openedUrls.push(args.url);
      return null;
    };
  });
  const link = page.locator(".cm-content").getByText("the spec");

  await link.hover();
  await expect(page.getByRole("tooltip")).toContainText("https://example.com/spec");

  await link.click({ modifiers: ["ControlOrMeta"] });
  await expect.poll(() => page.evaluate(() => window.openedUrls)).toEqual(["https://example.com/spec"]);

  await link.click();
  await expect(lines(page).first()).toHaveText("Read [the spec](https://example.com/spec) first");
  expect(await page.evaluate(() => window.openedUrls)).toHaveLength(1);
});

test("page links show as chips with the page's current title", async ({ page }) => {
  await page.goto("");
  const roadmap = await page.evaluate(() => window.fake.seedPage("Roadmap", "Q1 plans"));
  const grace = await page.evaluate(() => window.fake.addMember("Grace"));
  await page.evaluate(
    ([roadmap, grace]) =>
      window.fake.seedPage(
        "Notes",
        `See [old name](shy://page/${roadmap}) and shy://page/${roadmap}, ask [@Grace](shy://member/${grace})`,
      ),
    [roadmap, grace],
  );
  await openPage(page, "Notes");

  const chips = page.locator(".cm-content").getByRole("link", { name: "Roadmap" });
  await expect(chips).toHaveCount(2);
  await expect(lines(page).first()).toHaveText("See Roadmap and Roadmap, ask @Grace");

  await page.evaluate(
    (roadmap) => window.fake.handle("rename_page", { page: roadmap, title: "Roadmap 2027" }),
    roadmap,
  );
  await expect(page.locator(".cm-content").getByRole("link", { name: "Roadmap 2027" })).toHaveCount(2);

  await page.locator(".cm-content").getByRole("link", { name: "Roadmap 2027" }).first().click();
  await expect(page.getByLabel("Page title")).toHaveValue("Roadmap 2027");
});

test("two members editing one page converge, and the preview follows", async ({ page }) => {
  await seedAndOpen(page, "Shared", "# Title\n- [ ] one\n- [ ] two");
  await lines(page).nth(2).click();
  await page.keyboard.press("End");

  const remoteEdits = page.evaluate(async () => {
    for (let round = 0; round < 5; round += 1) {
      window.fake.receivePageEdit("Shared", { at: "# Title\n".length, insert: "Note **bold**\n" });
      await new Promise((resolve) => setTimeout(resolve, 40));
    }
  });
  await page.keyboard.type(" more words", { delay: 25 });
  await remoteEdits;

  const expected = `# Title\n${"Note **bold**\n".repeat(5)}- [ ] one\n- [ ] two more words`;
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Shared"))).toBe(expected);

  await page.getByLabel("Page title").focus();
  await expect(lines(page)).toHaveText(["Title", ...Array(5).fill("Note bold"), "one", "two more words"]);

  await page.getByRole("checkbox", { name: "two more words" }).click();
  await expect
    .poll(() => page.evaluate(() => window.fake.pageText("Shared")))
    .toBe(expected.replace("- [ ] two", "- [x] two"));
});

test("checking an empty item keeps the preview working", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await seedAndOpen(page, "Empty", "- [ ] \n\n**after**");
  await page.getByRole("checkbox", { name: "Checklist item" }).click();
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Empty"))).toBe("- [x] \n\n**after**");
  await expect(page.getByRole("checkbox", { name: "Checklist item" })).toBeChecked();
  await expect(lines(page).nth(2)).toHaveText("after");
  expect(errors).toEqual([]);
});
