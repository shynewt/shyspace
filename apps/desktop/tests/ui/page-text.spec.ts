/** The plain-text preview of a page, as page cards in chat show it. Runs without a browser. */
import { expect, test } from "@playwright/test";
import * as Y from "yjs";
import { PAGE_TEXT, pagePreview } from "../../src/features/pages/pageText";

function stateOf(text: string): Uint8Array {
  const doc = new Y.Doc();
  doc.getText(PAGE_TEXT).insert(0, text);
  return Y.encodeStateAsUpdate(doc);
}

test("a page preview keeps headings, paragraphs and items apart", () => {
  expect(pagePreview(stateOf("# Welcome to shyspace\n\nEverything here is **shared**.\n- [ ] first\n- second"))).toBe(
    "Welcome to shyspace · Everything here is shared. · first · second",
  );
});

test("a page preview drops markdown and stops at the length asked for", () => {
  expect(pagePreview(stateOf("> Read [the spec](https://example.com) and `run` it"))).toBe("Read the spec and run it");
  expect(pagePreview(stateOf("word ".repeat(100)), 20)).toBe("word word word word ");
});
