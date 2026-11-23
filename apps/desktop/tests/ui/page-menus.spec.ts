import { expect, type Page, test } from "@playwright/test";

const pageText = (page: Page) => page.evaluate(() => window.fake.pageText("Typing"));
const editor = (page: Page) => page.locator(".cm-content");

async function newPage(page: Page) {
  await page.getByRole("button", { name: "New page" }).click();
  await page.getByLabel("Page title").fill("Typing");
  await page.getByLabel("Page title").press("Enter");
  await expect(editor(page)).toBeFocused();
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
});

test("slash opens a block menu that filters as you type and runs with the keyboard", async ({ page }) => {
  await newPage(page);
  await page.keyboard.type("/");
  const menu = page.getByRole("listbox", { name: "Blocks" });
  await expect(menu.getByRole("option")).toHaveText([
    /Text/,
    /Heading 1/,
    /Heading 2/,
    /Heading 3/,
    /Bulleted list/,
    /Numbered list/,
    /Checklist/,
    /Quote/,
    /Callout/,
    /Code block/,
    /Table/,
    /Divider/,
    /Link to page/,
  ]);

  await page.keyboard.type("head");
  await expect(menu.getByRole("option")).toHaveText([/Heading 1/, /Heading 2/, /Heading 3/]);
  await page.keyboard.press("ArrowDown");
  const selected = menu.getByRole("option", { selected: true });
  await expect(selected).toHaveText(/Heading 2/);
  await expect(editor(page)).toHaveAttribute("aria-activedescendant", (await selected.getAttribute("id")) ?? "");

  await page.keyboard.press("Enter");
  await expect(menu).toHaveCount(0);
  await page.keyboard.type("Plan");
  await expect.poll(() => pageText(page)).toBe("## Plan");
});

test("a block turns the line's text into it", async ({ page }) => {
  await newPage(page);
  await page.keyboard.type("Buy milk /check");
  await page.keyboard.press("Enter");
  await expect.poll(() => pageText(page)).toBe("- [ ] Buy milk");

  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await page.keyboard.type("/code");
  await page.keyboard.press("Enter");
  await page.keyboard.type("let x = 1;");
  await expect.poll(() => pageText(page)).toBe("- [ ] Buy milk\n\n```\nlet x = 1;\n```");
});

test("callouts and tables start from a template around the line's text", async ({ page }) => {
  await newPage(page);
  await page.keyboard.type("Keys rotate on Friday /callout");
  await page.keyboard.press("Enter");
  await expect.poll(() => pageText(page)).toBe("> [!NOTE]\n> Keys rotate on Friday");

  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await page.keyboard.type("/table");
  await page.keyboard.press("Enter");
  await page.keyboard.type("Website");
  await expect.poll(() => pageText(page)).toContain("\n| Column 1 | Column 2 |\n| --- | --- |\n| Website |  |");
});

test("escape closes the menu and keeps what you typed", async ({ page }) => {
  await newPage(page);
  await page.keyboard.type("/quo");
  await expect(page.getByRole("listbox", { name: "Blocks" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("listbox", { name: "Blocks" })).toHaveCount(0);
  await page.keyboard.type("te");
  await expect(page.getByRole("listbox", { name: "Blocks" })).toHaveCount(0);
  await expect.poll(() => pageText(page)).toBe("/quote");
});

test("a slash inside a word is just a slash", async ({ page }) => {
  await newPage(page);
  await page.keyboard.type("and/or");
  await expect(page.getByRole("listbox")).toHaveCount(0);
});

test("at suggests people and inserts a mention", async ({ page }) => {
  const grace = await page.evaluate(() => window.fake.addMember("Grace Hopper"));
  await newPage(page);
  await page.keyboard.type("Ask @hop");
  const menu = page.getByRole("listbox", { name: "People to mention" });
  await expect(menu.getByRole("option")).toHaveText([/Grace Hopper/]);
  await page.keyboard.press("Enter");
  await page.keyboard.type("today");

  await expect.poll(() => pageText(page)).toBe(`Ask [@Grace Hopper](shy://member/${grace}) today`);
  await expect(editor(page).getByText("@Grace Hopper")).toBeVisible();
});

test("double brackets suggest pages and insert a link", async ({ page }) => {
  const roadmap = await page.evaluate(() => window.fake.seedPage("Roadmap", ""));
  await page.evaluate(() => window.fake.seedPage("Recipes", ""));
  await newPage(page);
  await page.keyboard.type("See [[road");
  const menu = page.getByRole("listbox", { name: "Pages to link" });
  await expect(menu.getByRole("option")).toHaveText([/Roadmap/]);
  await menu.getByRole("option", { name: /Roadmap/ }).click();

  await expect.poll(() => pageText(page)).toBe(`See [](shy://page/${roadmap}) `);
  await expect(editor(page).getByRole("link", { name: "Roadmap" })).toBeVisible();
});

for (const title of ["Plan \\", "Plan `v2", "Plan <https://evil.example>"]) {
  test(`a linked page titled ${JSON.stringify(title)} still shows as a page link`, async ({ page }) => {
    const target = await page.evaluate((title) => window.fake.seedPage(title, ""), title);
    await newPage(page);
    await page.keyboard.type("See [[plan");
    await page.getByRole("listbox", { name: "Pages to link" }).getByRole("option").first().click();

    // Titles come from other members, so the link carries only the page's id.
    await expect.poll(() => pageText(page)).toBe(`See [](shy://page/${target}) `);
    await expect(editor(page).getByRole("link", { name: title })).toBeVisible();
    await expect(editor(page).getByRole("link")).toHaveCount(1);
  });
}

test("link to page in the block menu moves on to picking the page", async ({ page }) => {
  const roadmap = await page.evaluate(() => window.fake.seedPage("Roadmap", ""));
  await newPage(page);
  await page.keyboard.type("/link");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("listbox", { name: "Pages to link" })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect.poll(() => pageText(page)).toBe(`[](shy://page/${roadmap}) `);
});

test("another member typing a slash doesn't open your menu", async ({ page }) => {
  await newPage(page);
  await page.keyboard.type("Hello ");
  await expect.poll(() => pageText(page)).toBe("Hello ");
  await page.evaluate(() => window.fake.receivePageEdit("Typing", { at: "Hello ".length, insert: "/" }));
  await expect.poll(() => pageText(page)).toBe("Hello /");
  await expect(page.getByRole("listbox")).toHaveCount(0);
});
