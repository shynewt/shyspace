import * as Y from "yjs";

export const PAGE_TEXT = "content";

/** Plain-text preview of a page's Yjs state: markdown markers removed, lines kept apart by " · ". */
export function pagePreview(state: Uint8Array, length = 180): string {
  const doc = new Y.Doc();
  if (state.length > 0) Y.applyUpdate(doc, state);
  const text = doc.getText(PAGE_TEXT).toString();
  doc.destroy();
  return text
    .split("\n")
    .map((line) =>
      line
        .replace(/^\s*(>\s*)*/, "")
        .replace(/^#{1,6}\s+/, "")
        .replace(/^[-+*]\s+(\[[ xX]\]\s*)?/, "")
        .replace(/[*_`~]|\[(.*?)\]\(.*?\)/g, "$1")
        .replace(/\s+/g, " ")
        .trim(),
    )
    .filter(Boolean)
    .join(" · ")
    .slice(0, length);
}
