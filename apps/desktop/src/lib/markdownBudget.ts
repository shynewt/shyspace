/**
 * Whether markdown is cheap enough to format. Formatting cost grows with a
 * text's structure, and some shapes grow far faster than their length: deep
 * nesting overflows the stack, a table costs columns × rows, and runs of
 * emphasis marks are quadratic. At these limits the worst case measured about
 * 150 ms (#148); above any of them, a text shows as plain text instead, so one
 * message can't freeze or blank the app.
 *
 * The check never tries to match the parser's own rules (where a code block
 * or a table ends): any mismatch would be a way around it. It counts every
 * line, and estimates tables on the high side. `remarkSizeLimits` is a second
 * net that checks what the parser actually built.
 */

/** Markdown files can be long; messages stop at 40,000 characters anyway. */
const MAX_CHARS = 50_000;
/** Code blocks count too; a longer paste reads better as a file (#172). */
const MAX_LINES = 1_000;
/** `*`, `_`, backticks, `[`, `]`, `!` and `~`, wherever they are. */
const MAX_MARKUP_CHARS = 2_000;
/** Quote and list levels at the start of one line. */
const MAX_NESTING = 20;
export const MAX_TABLE_CELLS = 3_000;
/** All tables in one text together. */
const MAX_TOTAL_TABLE_CELLS = 6_000;

const MARKUP = /[*_`[\]!~]/g;
const LIST_MARKER = /^(?:[-*+]|\d{1,9}[.)])(?=\s|$)/;

/** The parser ends lines at `\r\n`, `\r` or `\n`; so must everything that measures them. */
export function withPlainLineEnds(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

export function isCheapToFormat(text: string): boolean {
  if (text.length > MAX_CHARS) return false;
  const lines = withPlainLineEnds(text).split("\n");
  if (lines.length > MAX_LINES) return false;
  if ((text.match(MARKUP)?.length ?? 0) > MAX_MARKUP_CHARS) return false;
  if (lines.some((line) => nestingOf(line) > MAX_NESTING)) return false;
  const tables = tableCells(lines);
  return tables.largest <= MAX_TABLE_CELLS && tables.total <= MAX_TOTAL_TABLE_CELLS;
}

/**
 * Cells in the largest table and in all of them, counted on the high side:
 * every line from one with a pipe to the next blank line is a row, as wide
 * as the widest of them.
 */
function tableCells(lines: string[]): { largest: number; total: number } {
  let largest = 0;
  let total = 0;
  let current = 0;
  let rows = 0;
  let columns = 0;
  for (const line of lines) {
    if (line.trim() === "") {
      total += current;
      current = 0;
      rows = 0;
      columns = 0;
      continue;
    }
    const pipes = line.split("|").length - 1;
    if (rows === 0 && pipes === 0) continue;
    rows += 1;
    columns = Math.max(columns, pipes + 1);
    current = rows * columns;
    largest = Math.max(largest, current);
  }
  return { largest, total: total + current };
}

/**
 * Quote markers and list markers before a line's text. Indentation counts
 * only on a list line, where it nests lists; elsewhere it's just code.
 */
function nestingOf(line: string): number {
  let levels = 0;
  let spaces = 0;
  let hasListMarker = false;
  let index = 0;
  while (index < line.length && levels <= MAX_NESTING) {
    const character = line[index];
    if (character === " " || character === "\t") {
      spaces += character === "\t" ? 4 : 1;
      index += 1;
    } else if (character === ">") {
      levels += 1;
      index += 1;
    } else {
      const marker = LIST_MARKER.exec(line.slice(index, index + 11));
      if (!marker) break;
      levels += 1;
      hasListMarker = true;
      index += marker[0].length;
    }
  }
  return hasListMarker ? levels + Math.floor(spaces / 2) : levels;
}

/** Nodes in one parsed text; beyond this, drawing it gets slow whatever its shape. */
const MAX_NODES = 6_000;

type TreeNode = { type: string; children?: TreeNode[] };

/** What the parser built is too large to draw; shown as plain text by the message's error boundary. */
export class TooLargeToFormat extends Error {}

/**
 * A remark plugin that refuses a parsed text with too many nodes or too big
 * a table, before it becomes elements. A second net behind `isCheapToFormat`.
 */
export function remarkSizeLimits() {
  return (tree: TreeNode) => {
    let nodes = 0;
    let tableCells = 0;
    const pending: TreeNode[] = [tree];
    for (let node = pending.pop(); node; node = pending.pop()) {
      nodes += 1;
      if (nodes > MAX_NODES) throw new TooLargeToFormat("too many parts to format");
      const children = node.children ?? [];
      if (node.type === "table") {
        let columns = 0;
        for (const row of children) columns = Math.max(columns, row.children?.length ?? 0);
        if (columns * children.length > MAX_TABLE_CELLS) throw new TooLargeToFormat("a table too large to format");
        tableCells += columns * children.length;
        if (tableCells > MAX_TOTAL_TABLE_CELLS) throw new TooLargeToFormat("tables too large to format");
      }
      for (const child of children) pending.push(child);
    }
  };
}
