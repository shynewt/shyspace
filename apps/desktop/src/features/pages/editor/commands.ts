/** Formatting keys for pages: they write the markdown a person would type by hand. */
import { syntaxTree } from "@codemirror/language";
import { EditorSelection, type EditorState, type SelectionRange } from "@codemirror/state";
import type { Command, EditorView, KeyBinding } from "@codemirror/view";
import { linkAt, mentionAt } from "./links";
import { knownMembers } from "./livePreview";
import { openLink, openMember } from "./openLinks";
import { toggleTaskAt } from "./tasks";

type SyntaxNode = ReturnType<typeof syntaxTree>["topNode"];

const MARKS: Record<string, string> = {
  StrongEmphasis: "EmphasisMark",
  Emphasis: "EmphasisMark",
  InlineCode: "CodeMark",
};

/**
 * Wraps the selection in `marker`, or removes the formatting when the
 * selection is already inside it. With nothing selected it starts an empty
 * pair, or steps out past the closing mark when the cursor is right before it.
 */
function toggleInline(nodeName: string, marker: string): Command {
  return (view) => {
    const { state } = view;
    view.dispatch(
      state.changeByRange((range) => {
        const node = enclosing(state, range, nodeName);
        const marks = node?.getChildren(MARKS[nodeName]) ?? [];
        if (node && marks.length >= 2) {
          const [open, close] = [marks[0], marks[marks.length - 1]];
          if (range.empty && range.head === close.from) return { range: EditorSelection.cursor(close.to) };
          const openLength = open.to - open.from;
          const keep = (position: number) => Math.min(Math.max(position, open.to), close.from) - openLength;
          return {
            changes: [
              { from: open.from, to: open.to },
              { from: close.from, to: close.to },
            ],
            range: EditorSelection.range(keep(range.anchor), keep(range.head)),
          };
        }
        const { from, to } = trimmed(state, range);
        if (from === to) {
          return { changes: { from, insert: marker + marker }, range: EditorSelection.cursor(from + marker.length) };
        }
        return {
          changes: [
            { from, insert: marker },
            { from: to, insert: marker },
          ],
          range: EditorSelection.range(from + marker.length, to + marker.length),
        };
      }),
      { userEvent: "input.format", scrollIntoView: true },
    );
    return true;
  };
}

/** The innermost `name` node holding the whole range. */
function enclosing(state: EditorState, range: SelectionRange, name: string): SyntaxNode | null {
  for (const side of [1, -1] as const) {
    for (let node: SyntaxNode | null = syntaxTree(state).resolveInner(range.from, side); node; node = node.parent) {
      if (node.name === name && node.from <= range.from && node.to >= range.to) return node;
    }
  }
  return null;
}

/** Markdown marks must touch the text, so selected spaces at either end stay outside. */
function trimmed(state: EditorState, range: SelectionRange): { from: number; to: number } {
  const text = state.sliceDoc(range.from, range.to);
  const from = range.from + (text.length - text.trimStart().length);
  return { from, to: Math.max(from, range.to - (text.length - text.trimEnd().length)) };
}

export const toggleBold = toggleInline("StrongEmphasis", "**");
export const toggleItalic = toggleInline("Emphasis", "*");
export const toggleCode = toggleInline("InlineCode", "`");

/** Makes the selected text a link and puts the cursor where the address goes. */
export const linkSelection: Command = (view) => {
  const range = view.state.selection.main;
  if (range.empty) return false;
  const text = view.state.sliceDoc(range.from, range.to);
  if (/^(https?:\/\/|www\.)\S+$/i.test(text)) {
    view.dispatch({
      changes: { from: range.from, to: range.to, insert: `[](${text})` },
      selection: { anchor: range.from + 1 },
      userEvent: "input.format",
    });
    return true;
  }
  const insert = `[${text}]()`;
  view.dispatch({
    changes: { from: range.from, to: range.to, insert },
    selection: { anchor: range.from + insert.length - 1 },
    userEvent: "input.format",
  });
  return true;
};

/** Opens the link or mentioned member's card at the cursor, or checks off the checklist item on its line. */
export const openLinkOrToggleTask: Command = (view: EditorView) => {
  const { head } = view.state.selection.main;
  const memberId = mentionAt(view.state, head);
  const spot = view.coordsAtPos(head);
  if (memberId && spot && view.state.field(knownMembers).has(memberId)) {
    const cursorBox = new DOMRect(spot.left, spot.top, 0, spot.bottom - spot.top);
    openMember(view, memberId, { getBoundingClientRect: () => cursorBox }, "keyboard");
    return true;
  }
  const link = linkAt(view.state, head);
  if (link) {
    openLink(view, link);
    return true;
  }
  return toggleTaskAt(view, head);
};

export const formattingKeymap: readonly KeyBinding[] = [
  { key: "Mod-b", run: toggleBold },
  { key: "Mod-i", run: toggleItalic },
  { key: "Mod-e", run: toggleCode },
  // Only with a selection: otherwise the key stays the app's search.
  { key: "Mod-k", run: linkSelection, stopPropagation: true },
  { key: "Mod-Enter", run: openLinkOrToggleTask },
];
