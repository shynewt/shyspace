/**
 * Callouts: a quote whose first line is `[!TIP]` (GitHub's and Obsidian's
 * syntax) shows as a coloured box with a label. Messages and pages both use
 * these kinds; their colours are the `callout-<kind>` classes in styles.css.
 */
import type { ExtraProps } from "react-markdown";

export type CalloutKind = "note" | "tip" | "important" | "warning" | "caution";

/** Lucide's icons as path data, so plain-DOM editor widgets can draw them too. */
export const CALLOUTS: Record<CalloutKind, { label: string; iconPaths: readonly string[] }> = {
  note: {
    label: "Note",
    iconPaths: ["M2 12a10 10 0 1 0 20 0a10 10 0 1 0-20 0", "M12 16v-4", "M12 8h.01"],
  },
  tip: {
    label: "Tip",
    iconPaths: [
      "M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5",
      "M9 18h6",
      "M10 22h4",
    ],
  },
  important: {
    label: "Important",
    iconPaths: [
      "M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z",
      "M12 15h.01",
      "M12 7v4",
    ],
  },
  warning: {
    label: "Warning",
    iconPaths: ["m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3", "M12 9v4", "M12 17h.01"],
  },
  caution: {
    label: "Caution",
    iconPaths: [
      "M12 16h.01",
      "M12 8v4",
      "M15.312 2a2 2 0 0 1 1.414.586l4.688 4.688A2 2 0 0 1 22 8.688v6.624a2 2 0 0 1-.586 1.414l-4.688 4.688a2 2 0 0 1-1.414.586H8.688a2 2 0 0 1-1.414-.586l-4.688-4.688A2 2 0 0 1 2 15.312V8.688a2 2 0 0 1 .586-1.414l4.688-4.688A2 2 0 0 1 8.688 2z",
    ],
  },
};

export function isCalloutKind(name: unknown): name is CalloutKind {
  return typeof name === "string" && Object.hasOwn(CALLOUTS, name);
}

/** `[!INFO]` is Obsidian's name for a note. */
const KIND_BY_NAME = new Map<string, CalloutKind>([
  ["note", "note"],
  ["info", "note"],
  ["tip", "tip"],
  ["important", "important"],
  ["warning", "warning"],
  ["caution", "caution"],
]);
const MARKER = /^\[!([a-z]+)\][ \t]*/i;

/** The callout a quote's first line starts, and how long its `[!KIND]` marker is. */
export function calloutMarker(firstLine: string): { kind: CalloutKind; length: number } | null {
  const match = firstLine.match(MARKER);
  const kind = match && KIND_BY_NAME.get(match[1].toLowerCase());
  return match && kind ? { kind, length: match[0].length } : null;
}

type MarkdownElement = NonNullable<ExtraProps["node"]>;
type MarkdownContent = MarkdownElement["children"][number];

/**
 * A rehype step for rendered markdown: each callout quote becomes an
 * `<aside data-callout="tip">` without its marker, for the page to draw.
 */
export function rehypeCallouts() {
  return (tree: { children: MarkdownContent[] }) => {
    for (const child of tree.children) markCallouts(child);
  };
}

function markCallouts(node: MarkdownContent) {
  if (node.type !== "element") return;
  for (const child of node.children) markCallouts(child);
  if (node.tagName !== "blockquote") return;
  const paragraph = node.children.find((child) => child.type === "element");
  if (paragraph?.type !== "element" || paragraph.tagName !== "p") return;
  const first = paragraph.children[0];
  if (first?.type !== "text") return;
  const marker = calloutMarker(first.value);
  if (!marker) return;
  first.value = first.value.slice(marker.length).replace(/^\n/, "");
  if (!first.value) dropLeadingBreak(paragraph);
  if (!paragraph.children.length) node.children.splice(node.children.indexOf(paragraph), 1);
  node.tagName = "aside";
  node.properties = { ...node.properties, dataCallout: marker.kind };
}

/** Removes the emptied marker and the line break after it. */
function dropLeadingBreak(paragraph: MarkdownElement) {
  paragraph.children.shift();
  const next = paragraph.children[0];
  if (next?.type === "element" && next.tagName === "br") paragraph.children.shift();
  const after = paragraph.children[0];
  if (after?.type === "text") {
    after.value = after.value.replace(/^\n/, "");
    if (!after.value) paragraph.children.shift();
  }
}
