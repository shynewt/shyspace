/**
 * The editor side of the page menus: `/` for blocks, `@` for people, `[[`
 * for pages. This finds what is being typed and owns the menu keys; the
 * menu itself is React (`SuggestionMenu.tsx`).
 */
import { syntaxTree } from "@codemirror/language";
import { type EditorState, type Extension, Prec, StateEffect, StateField } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { mentionQueryAt } from "../../../lib/mentions";

type SyntaxNode = ReturnType<typeof syntaxTree>["topNode"];

export type SuggestionKind = "command" | "member" | "page";

/** An open menu: the trigger and query span `from` to `to`, which is the cursor. */
export type Suggestion = { kind: SuggestionKind; from: number; to: number; query: string };

const PAGE_TRIGGER = /\[\[([^[\]]*)$/;
const COMMAND_TRIGGER = /(?:^|\s)\/([\p{L}\p{N}]+(?: [\p{L}\p{N}]+)*)?$/u;
const CODE = new Set(["InlineCode", "FencedCode", "CodeBlock", "CodeText"]);

/** What `textBeforeCursor` (on one line) is asking a menu for, if anything. */
export function suggestionTrigger(
  textBeforeCursor: string,
): { kind: SuggestionKind; start: number; query: string } | null {
  const page = PAGE_TRIGGER.exec(textBeforeCursor);
  if (page) return { kind: "page", start: page.index, query: page[1] };
  const member = mentionQueryAt(textBeforeCursor, textBeforeCursor.length);
  if (member) return { kind: "member", start: member.start, query: member.query };
  const command = COMMAND_TRIGGER.exec(textBeforeCursor);
  if (command) {
    const query = command[1] ?? "";
    return { kind: "command", start: textBeforeCursor.length - query.length - 1, query };
  }
  return null;
}

function suggestionAtCursor(state: EditorState): Suggestion | null {
  const { selection } = state;
  if (selection.ranges.length > 1 || !selection.main.empty) return null;
  const cursor = selection.main.head;
  const line = state.doc.lineAt(cursor);
  const trigger = suggestionTrigger(line.text.slice(0, cursor - line.from));
  if (!trigger) return null;
  for (let node: SyntaxNode | null = syntaxTree(state).resolveInner(cursor, -1); node; node = node.parent) {
    if (CODE.has(node.name)) return null;
  }
  return { kind: trigger.kind, from: line.from + trigger.start, to: cursor, query: trigger.query };
}

export const closeSuggestions = StateEffect.define<null>();
/** Opens a menu at a trigger written by a command rather than typed. */
export const openSuggestions = StateEffect.define<{ kind: SuggestionKind; from: number }>();

/**
 * The open menu. It opens only when you type its trigger, so moving into old
 * text or another member's edit never pops one up, and stays open while the
 * cursor stays in the same trigger.
 */
export const suggestionState = StateField.define<Suggestion | null>({
  create: () => null,
  update(open, transaction) {
    let opened: { kind: SuggestionKind; from: number } | null = null;
    for (const effect of transaction.effects) {
      if (effect.is(closeSuggestions)) return null;
      if (effect.is(openSuggestions)) opened = effect.value;
    }
    const current = suggestionAtCursor(transaction.state);
    if (!current) return null;
    const isSame = (wanted: { kind: SuggestionKind; from: number } | null) =>
      wanted !== null && wanted.kind === current.kind && wanted.from === current.from;
    if (isSame(opened)) return current;
    if (open && isSame({ kind: open.kind, from: transaction.changes.mapPos(open.from) })) return current;
    const isJustTyped = transaction.docChanged && transaction.isUserEvent("input.type") && current.query === "";
    return isJustTyped ? current : null;
  },
});

const MENU_KEYS = ["ArrowDown", "ArrowUp", "Enter", "Tab", "Escape"] as const;
export type MenuKey = (typeof MENU_KEYS)[number];

/**
 * Menus for pages: `onKey` gets the menu keys while a menu is open and says
 * whether it used them; `onChange` hears when the menu opens, changes or
 * closes. Nothing is open while the editor isn't focused.
 */
export function suggestions({
  onKey,
  onChange,
}: {
  onKey: (key: MenuKey, view: EditorView) => boolean;
  onChange: (suggestion: Suggestion | null) => void;
}): Extension {
  return [
    suggestionState,
    Prec.highest(
      keymap.of(
        MENU_KEYS.map((key) => ({
          key,
          run: (view: EditorView) => view.state.field(suggestionState) !== null && !view.composing && onKey(key, view),
        })),
      ),
    ),
    EditorView.updateListener.of((update) => {
      const isChanged =
        update.focusChanged || update.startState.field(suggestionState) !== update.state.field(suggestionState);
      if (isChanged) onChange(update.view.hasFocus ? update.state.field(suggestionState) : null);
    }),
  ];
}
