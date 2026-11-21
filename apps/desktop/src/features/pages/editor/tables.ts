/**
 * Tables in the live preview: a table nobody is editing shows as a real
 * table, and clicking a cell puts the cursor there; with the cursor inside,
 * its markdown shows instead, in monospace so the columns line up. A state
 * field draws them because CodeMirror takes block widgets only from one.
 */
import { markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxTree } from "@codemirror/language";
import { type EditorState, type Range, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";

type SyntaxNode = ReturnType<typeof syntaxTree>["topNode"];
type Alignment = "left" | "center" | "right" | null;
/** A cell's text and where it starts in the page. */
type Cell = { text: string; from: number };

const setFocused = StateEffect.define<boolean>();

const tableState = StateField.define<{ isFocused: boolean; decorations: DecorationSet }>({
  create: (state) => ({ isFocused: false, decorations: tableDecorations(state, false) }),
  update: (value, transaction) => {
    let { isFocused } = value;
    for (const effect of transaction.effects) if (effect.is(setFocused)) isFocused = effect.value;
    const isUnchanged =
      isFocused === value.isFocused &&
      !transaction.docChanged &&
      !transaction.selection &&
      syntaxTree(transaction.startState) === syntaxTree(transaction.state);
    if (isUnchanged) return value;
    return { isFocused, decorations: tableDecorations(transaction.state, isFocused) };
  },
  provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
});

export const tablePreview = [
  tableState,
  EditorView.focusChangeEffect.of((_state, isFocusing) => setFocused.of(isFocusing)),
];

function tableDecorations(state: EditorState, isFocused: boolean): DecorationSet {
  const decorations: Range<Decoration>[] = [];
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name === "Document") return;
      if (node.name !== "Table") return false;
      const from = state.doc.lineAt(node.from).from;
      const to = state.doc.lineAt(node.to).to;
      const isTopLevel = node.node.parent?.name === "Document";
      const isEditing = isFocused && state.selection.ranges.some((range) => range.from <= to && range.to >= from);
      if (isTopLevel && !isEditing) {
        const widget = new TableWidget(state.doc.sliceString(from, to), from);
        decorations.push(Decoration.replace({ widget, block: true }).range(from, to));
        return false;
      }
      for (let line = state.doc.lineAt(from).number; line <= state.doc.lineAt(to).number; line += 1) {
        decorations.push(Decoration.line({ class: "cm-md-table-source" }).range(state.doc.line(line).from));
      }
      return false;
    },
  });
  return Decoration.set(decorations, true);
}

class TableWidget extends WidgetType {
  constructor(
    readonly source: string,
    readonly from: number,
  ) {
    super();
  }

  eq(other: TableWidget) {
    return other.source === this.source && other.from === this.from;
  }

  /** Clicks are the widget's own: a cell moves the cursor into its text. */
  ignoreEvent() {
    return true;
  }

  toDOM(view: EditorView) {
    const lines: Cell[][] = [];
    let offset = this.from;
    for (const line of this.source.split("\n")) {
      lines.push(cellsOf(line, offset));
      offset += line.length + 1;
    }
    const [header = [], delimiter = [], ...rows] = lines;
    const alignments = delimiter.map((cell) => alignmentOf(cell.text));

    const wrapper = document.createElement("div");
    wrapper.className = "cm-md-table-wrap";
    const table = document.createElement("table");
    table.className = "cm-md-table";
    const head = table.createTHead().insertRow();
    for (const [index, cell] of header.entries()) head.append(this.cell(view, "th", cell, alignments[index]));
    const body = table.createTBody();
    for (const row of rows) {
      const tableRow = body.insertRow();
      for (let index = 0; index < header.length; index += 1) {
        tableRow.append(this.cell(view, "td", row[index], alignments[index]));
      }
    }
    wrapper.append(table);
    return wrapper;
  }

  private cell(view: EditorView, tag: "th" | "td", cell: Cell | undefined, alignment: Alignment | undefined) {
    const element = document.createElement(tag);
    if (alignment) element.style.textAlign = alignment;
    if (!cell) return element;
    appendInline(element, cell.text);
    element.addEventListener("mousedown", (event) => {
      event.preventDefault();
      view.dispatch({ selection: { anchor: cell.from } });
      view.focus();
    });
    return element;
  }
}

/** A row's cells, split on pipes that aren't escaped, without the optional outer pipes. */
export function cellsOf(line: string, lineFrom: number): Cell[] {
  const parts: { text: string; start: number }[] = [];
  let start = 0;
  for (let index = 0; index < line.length; index += 1) {
    if (line[index] === "\\") index += 1;
    else if (line[index] === "|") {
      parts.push({ text: line.slice(start, index), start });
      start = index + 1;
    }
  }
  parts.push({ text: line.slice(start), start });
  if (line.trimStart().startsWith("|")) parts.shift();
  if (line.trimEnd().endsWith("|") && parts.length > 0) parts.pop();
  return parts.map(({ text, start: partStart }) => {
    const leading = text.length - text.trimStart().length;
    return { text: text.trim(), from: lineFrom + partStart + leading };
  });
}

function alignmentOf(delimiter: string): Alignment {
  const isLeft = delimiter.startsWith(":");
  const isRight = delimiter.endsWith(":");
  if (isLeft && isRight) return "center";
  if (isRight) return "right";
  if (isLeft) return "left";
  return null;
}

const INLINE_TAGS = new Map<string, { tag: string; className?: string }>([
  ["StrongEmphasis", { tag: "strong" }],
  ["Emphasis", { tag: "em" }],
  ["Strikethrough", { tag: "s" }],
  ["InlineCode", { tag: "code", className: "cm-md-code" }],
  ["Link", { tag: "span", className: "cm-md-link" }],
]);
const MARKS = new Set(["EmphasisMark", "StrikethroughMark", "CodeMark", "LinkMark", "URL", "LinkTitle", "LinkLabel"]);

/** A cell's inline markdown as formatted text; nothing from it is ever parsed as HTML. */
function appendInline(parent: HTMLElement, text: string) {
  const paragraph = markdownLanguage.parser.parse(text).topNode.firstChild;
  if (paragraph) appendChildren(parent, paragraph, text);
  else parent.append(text);
}

function appendChildren(parent: HTMLElement, node: SyntaxNode, text: string) {
  let at = node.from;
  for (let child = node.firstChild; child; child = child.nextSibling) {
    if (child.from > at) parent.append(text.slice(at, child.from));
    appendNode(parent, child, text);
    at = child.to;
  }
  if (node.to > at) parent.append(text.slice(at, node.to));
}

function appendNode(parent: HTMLElement, node: SyntaxNode, text: string) {
  if (MARKS.has(node.name)) return;
  if (node.name === "Escape") {
    parent.append(text.slice(node.from + 1, node.to));
    return;
  }
  const inline = INLINE_TAGS.get(node.name);
  if (!inline) {
    appendChildren(parent, node, text);
    return;
  }
  const element = document.createElement(inline.tag);
  if (inline.className) element.className = inline.className;
  appendChildren(element, node, text);
  parent.append(element);
}
