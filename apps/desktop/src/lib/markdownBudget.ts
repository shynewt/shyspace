/**
 * Whether markdown is cheap enough to format. Formatting cost grows with a
 * text's structure, and some shapes grow far faster than their length: deep
 * nesting overflows the stack, a table costs columns × rows, and runs of
 * emphasis marks are quadratic. At these limits the worst case measured about
 * 100 ms (#148); above any of them, a text shows as plain text instead, so one
 * message can't freeze or blank the app. Code blocks don't count: their
 * contents aren't parsed.
 */

/** Markdown files can be long; messages stop at 40,000 characters anyway. */
const MAX_CHARS = 50_000;
const MAX_LINES = 500;
/** `*`, `_`, backticks, `[` and `~` outside code blocks. */
const MAX_MARKUP_CHARS = 1_500;
/** Quote and list levels at the start of one line. */
const MAX_NESTING = 20;
const MAX_TABLE_CELLS = 3_000;

const FENCE = /^\s*(```|~~~)/;
const MARKUP = /[*_`[~]/g;
const LIST_MARKER = /^(?:[-*+]|\d{1,9}[.)])(?=\s|$)/;

export function isCheapToFormat(text: string): boolean {
  if (text.length > MAX_CHARS) return false;
  let lines = 0;
  let markupChars = 0;
  let isInCode = false;
  let tableColumns = 0;
  let tableRows = 0;
  for (const line of text.split("\n")) {
    if (FENCE.test(line)) {
      isInCode = !isInCode;
      continue;
    }
    if (isInCode) continue;
    if (line.trim() === "") {
      tableRows = 0;
      continue;
    }
    lines += 1;
    markupChars += line.match(MARKUP)?.length ?? 0;
    if (lines > MAX_LINES || markupChars > MAX_MARKUP_CHARS || nestingOf(line) > MAX_NESTING) return false;
    if (line.includes("|")) {
      if (tableRows === 0) tableColumns = line.split("|").length - 1;
      tableRows += 1;
      if (tableColumns * tableRows > MAX_TABLE_CELLS) return false;
    } else {
      tableRows = 0;
    }
  }
  return true;
}

/** Quote markers, list markers and every two spaces of indent before a line's text. */
function nestingOf(line: string): number {
  let levels = 0;
  let spaces = 0;
  let rest = line;
  while (rest.length > 0) {
    const first = rest[0];
    if (first === " " || first === "\t") {
      spaces += first === "\t" ? 4 : 1;
      rest = rest.slice(1);
    } else if (first === ">") {
      levels += 1;
      rest = rest.slice(1);
    } else {
      const marker = rest.match(LIST_MARKER);
      if (!marker) break;
      levels += 1;
      rest = rest.slice(marker[0].length);
    }
    if (levels + Math.floor(spaces / 2) > MAX_NESTING) break;
  }
  return levels + Math.floor(spaces / 2);
}
