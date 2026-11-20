/** Text helpers behind the page editor's menus. Run without a browser. */
import { expect, test } from "@playwright/test";
import { lineAsBlock } from "../../src/features/pages/editor/blocks";
import { suggestionTrigger } from "../../src/features/pages/editor/suggestions";

test("a line turned into a block keeps its text and drops its old marker", () => {
  expect(lineAsBlock("Plan", "heading1")).toBe("# Plan");
  expect(lineAsBlock("## Plan", "checklist")).toBe("- [ ] Plan");
  expect(lineAsBlock("- [x] Plan", "quote")).toBe("> Plan");
  expect(lineAsBlock("> Plan", "numbered")).toBe("1. Plan");
  expect(lineAsBlock("3. Plan ", "text")).toBe("Plan");
  expect(lineAsBlock("  - nested", "checklist")).toBe("  - [ ] nested");
  expect(lineAsBlock("  - nested", "heading2")).toBe("## nested");
  expect(lineAsBlock("", "bullet")).toBe("- ");
});

test("menus open on a slash, at sign or double bracket in the right place", () => {
  expect(suggestionTrigger("/")).toEqual({ kind: "command", start: 0, query: "" });
  expect(suggestionTrigger("Buy milk /chec")).toEqual({ kind: "command", start: 9, query: "chec" });
  expect(suggestionTrigger("/heading 1")).toEqual({ kind: "command", start: 0, query: "heading 1" });
  expect(suggestionTrigger("and/or")).toBeNull();
  expect(suggestionTrigger("/ ")).toBeNull();
  expect(suggestionTrigger("Ask @Gra")).toEqual({ kind: "member", start: 4, query: "Gra" });
  expect(suggestionTrigger("mail me@host")).toBeNull();
  expect(suggestionTrigger("See [[Road map")).toEqual({ kind: "page", start: 4, query: "Road map" });
  expect(suggestionTrigger("See [[done]] now")).toBeNull();
});

test("a long line without a trigger is checked quickly", () => {
  const started = performance.now();
  expect(suggestionTrigger(`${"word ".repeat(20_000)}x!`)).toBeNull();
  expect(performance.now() - started).toBeLessThan(200);
});
