import { expect, test } from "@playwright/test";
import { isCheapToFormat, remarkSizeLimits } from "../../src/lib/markdownBudget";

/** Shapes that froze or crashed the formatter (#148), each ending in a marker to find. */
const TOO_COSTLY = {
  "deeply nested quotes": `${">".repeat(5_000)} still here`,
  "deeply nested lists": `${"- ".repeat(2_000)}still here`,
  "a huge empty table": `|${"a|".repeat(1_500)}\n|${"-|".repeat(1_500)}\n${"|\n".repeat(1_500)}still here`,
  "a long run of emphasis marks": `${"_a*".repeat(10_000)} still here`,
  "thousands of list items": `${"- item\n".repeat(5_000)}still here`,
  // Ways around an earlier check that guessed where code blocks and tables end (#148 re-check).
  "a mixed fence before a table": `\`\`\`\n~~~\n\`\`\`\n${wideTable("|\n")}still here`,
  "an indented fence before nesting": `    \`\`\`\n${">".repeat(5_000)} still here`,
  "a backtick fence with a backtick in its info": `\`\`\` \`\n${"_a*".repeat(6_000)} still here`,
  "bare carriage returns": `${wideTable("|\r").replaceAll("\n", "\r")}still here`,
  "table rows without pipes": `${wideTable("x\n")}still here`,
  "a one-pipe line above a wide table": `a|b\n${wideTable("|\n")}still here`,
};

/** A header 1,500 columns wide, then 490 rows of `row`: about a million cells once padded. */
function wideTable(row: string): string {
  return `|${"a|".repeat(1_500)}\n|${"-|".repeat(1_500)}\n${row.repeat(490)}`;
}

test.describe("the formatting budget", () => {
  for (const [shape, text] of Object.entries(TOO_COSTLY)) {
    test(`${shape} is too costly to format`, () => {
      expect(isCheapToFormat(text)).toBe(false);
    });
  }

  test("ordinary messages, even long ones, are formatted", () => {
    const prose = "the quick brown fox jumps over the lazy dog ".repeat(900).slice(0, 40_000);
    const table = `| a | b | c | d | e | f |\n|---|---|---|---|---|---|\n${"| 1 | 2 | 3 | 4 | 5 | 6 |\n".repeat(200)}`;
    const code = `\`\`\`ts\n${"const total = items.map((item) => item.price * item.count);\n".repeat(600)}\`\`\``;
    const windowsLines = "first line\r\nsecond **line**\r\n- a\r\n- b\r\n";
    const notes = "# Plan\n\n> **Note:** keep it _short_.\n\n- [ ] one\n- [x] two\n  - nested `code`\n";
    for (const text of [prose, table, code, notes, windowsLines]) expect(isCheapToFormat(text)).toBe(true);
  });
});

test.describe("the parsed-size net", () => {
  const leaf = { type: "text" };
  const refuses = (tree: object) => () =>
    remarkSizeLimits()(tree as Parameters<ReturnType<typeof remarkSizeLimits>>[0]);

  test("refuses a text the parser made into too many parts", () => {
    expect(refuses({ type: "root", children: Array.from({ length: 7_000 }, () => leaf) })).toThrow();
  });

  test("refuses a table too large, however its rows were written", () => {
    const row = { type: "tableRow", children: Array.from({ length: 100 }, () => ({ type: "tableCell" })) };
    const table = { type: "table", children: Array.from({ length: 40 }, () => row) };
    expect(refuses({ type: "root", children: [table] })).toThrow();
  });

  test("lets an ordinary text through", () => {
    const row = { type: "tableRow", children: Array.from({ length: 6 }, () => ({ type: "tableCell" })) };
    const table = { type: "table", children: Array.from({ length: 200 }, () => row) };
    expect(refuses({ type: "root", children: [table, leaf, leaf] })).not.toThrow();
  });
});

test.describe("in the app", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("");
    await page.getByRole("button", { name: "general", exact: true }).click();
    await page.evaluate(() => window.fake.addMember("Grace"));
  });

  for (const [shape, text] of Object.entries(TOO_COSTLY)) {
    test(`a message with ${shape} shows as plain text and the app keeps working`, async ({ page }) => {
      await page.evaluate(
        (text) => window.fake.receiveMessage(window.fake.memberNamed("Grace"), "general", text),
        text,
      );
      const message = page.getByRole("article").filter({ hasText: "still here" });
      await expect(message).toBeVisible();
      await expect(message.locator("blockquote, li, table, em")).toHaveCount(0);

      const composer = page.getByLabel("Message #general");
      await composer.fill("still working");
      await composer.press("Enter");
      await expect(page.getByRole("article").filter({ hasText: "still working" })).toBeVisible();
    });
  }

  test("a message the parser makes too large still shows as plain text", async ({ page }) => {
    // Bare links need no markup characters, so only the parsed-size net catches thousands of them.
    const links = `${"https://a.b ".repeat(3_300)}still here`;
    await page.evaluate((text) => window.fake.receiveMessage(window.fake.memberNamed("Grace"), "general", text), links);
    const message = page.getByRole("article").filter({ hasText: "still here" });
    await expect(message).toBeVisible();
    await expect(message.getByRole("link")).toHaveCount(0);
  });

  test("a markdown file too costly to format previews as plain text", async ({ page }) => {
    await page.evaluate((text) => {
      const grace = window.fake.memberNamed("Grace");
      const message = window.fake.receiveMessage(grace, "general", "Notes");
      window.fake.files.receive(grace, message, {
        name: "nested.md",
        type: "text/markdown",
        bytes: Array.from(new TextEncoder().encode(text)),
      });
    }, TOO_COSTLY["deeply nested quotes"]);
    await page.getByRole("button", { name: "View nested.md" }).click();
    const preview = page.getByRole("dialog", { name: "nested.md" });
    await expect(preview.getByText("still here")).toBeVisible();
    await expect(preview.locator("blockquote")).toHaveCount(0);
  });
});
