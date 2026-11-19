import * as Y from "yjs";

export const PAGE_TEXT = "content";

/** Plain-text preview of a page's Yjs state, markdown markers removed. */
export function pagePreview(state: Uint8Array, length = 180): string {
  const doc = new Y.Doc();
  if (state.length > 0) Y.applyUpdate(doc, state);
  const text = doc.getText(PAGE_TEXT).toString();
  doc.destroy();
  return text
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/[*_`>~]|\[(.*?)\]\(.*?\)/g, "$1")
    .replace(/^\s*[-+]\s+(\[[ x]\]\s*)?/gm, "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, length);
}
