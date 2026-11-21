/** The blocks the slash menu inserts, each written as the markdown a person would type. */
import type { EditorView } from "@codemirror/view";

export type LineBlock = "text" | "heading1" | "heading2" | "heading3" | "bullet" | "numbered" | "checklist" | "quote";
export type BlockKind = LineBlock | "callout" | "code" | "table" | "divider";

export type Block = { kind: BlockKind; label: string; hint: string; keywords: string };

export const BLOCKS: readonly Block[] = [
  { kind: "text", label: "Text", hint: "", keywords: "paragraph plain" },
  { kind: "heading1", label: "Heading 1", hint: "#", keywords: "h1 title" },
  { kind: "heading2", label: "Heading 2", hint: "##", keywords: "h2 subtitle" },
  { kind: "heading3", label: "Heading 3", hint: "###", keywords: "h3" },
  { kind: "bullet", label: "Bulleted list", hint: "-", keywords: "ul unordered bullets" },
  { kind: "numbered", label: "Numbered list", hint: "1.", keywords: "ol ordered numbers" },
  { kind: "checklist", label: "Checklist", hint: "[ ]", keywords: "todo task checkbox" },
  { kind: "quote", label: "Quote", hint: ">", keywords: "blockquote citation" },
  { kind: "callout", label: "Callout", hint: "> [!NOTE]", keywords: "note info tip important warning caution alert" },
  { kind: "code", label: "Code block", hint: "```", keywords: "snippet pre" },
  { kind: "table", label: "Table", hint: "| |", keywords: "grid columns rows" },
  { kind: "divider", label: "Divider", hint: "---", keywords: "hr rule line separator" },
];

const PREFIXES: Record<LineBlock, string> = {
  text: "",
  heading1: "# ",
  heading2: "## ",
  heading3: "### ",
  bullet: "- ",
  numbered: "1. ",
  checklist: "- [ ] ",
  quote: "> ",
};

const NESTABLE: readonly LineBlock[] = ["bullet", "numbered", "checklist"];
const BLOCK_PREFIX = /^(#{1,6}[ \t]+|>[ \t]?|[-*+][ \t]+\[[ xX]\][ \t]+|[-*+][ \t]+|\d+[.)][ \t]+)/;

/** The line as a `kind` block, keeping its text: "## Plan" as a checklist is "- [ ] Plan". */
export function lineAsBlock(line: string, kind: LineBlock): string {
  const indent = line.match(/^[ \t]*/)?.[0] ?? "";
  const text = line.slice(indent.length).replace(BLOCK_PREFIX, "").trimEnd();
  return (NESTABLE.includes(kind) ? indent : "") + PREFIXES[kind] + text;
}

const TABLE_HEADER = "| Column 1 | Column 2 |\n| --- | --- |\n| ";

/**
 * Removes the `/query` typed between `from` and `to`, then makes its line a
 * `kind` block. Callouts, code blocks, tables and dividers span lines of their own;
 * the line's text goes in the first one you'd type in.
 */
export function insertBlock(view: EditorView, from: number, to: number, kind: BlockKind): void {
  const { doc } = view.state;
  const line = doc.lineAt(from);
  const rest = line.text.slice(0, from - line.from) + line.text.slice(to - line.from);
  const text = lineAsBlock(rest, "text");
  let insert: string;
  let cursor: number;
  if (kind === "code") {
    insert = `\`\`\`\n${text}\n\`\`\``;
    cursor = line.from + 4 + text.length;
  } else if (kind === "callout") {
    insert = `> [!NOTE]\n> ${text}`;
    cursor = line.from + insert.length;
  } else if (kind === "table") {
    insert = `${TABLE_HEADER}${text} |  |`;
    cursor = line.from + TABLE_HEADER.length + text.length;
  } else if (kind === "divider") {
    // `---` right under text would make that text a heading instead.
    const isAfterText = line.number > 1 && doc.line(line.number - 1).text.trim() !== "";
    const divider = `${isAfterText && !text ? "\n" : ""}---\n`;
    insert = text ? `${rest.trimEnd()}\n\n${divider}` : divider;
    cursor = line.from + insert.length;
  } else {
    insert = lineAsBlock(rest, kind);
    cursor = line.from + insert.length;
  }
  view.dispatch({
    changes: { from: line.from, to: line.to, insert },
    selection: { anchor: cursor },
    userEvent: "input.complete",
    scrollIntoView: true,
  });
}
