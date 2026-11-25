import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";

/** Marks the line at a position as the one a link led to; null clears it. */
export const showMentionTarget = StateEffect.define<number | null>();

const targetLine = Decoration.line({ class: "cm-mention-target" });

/**
 * The line a notification or inbox entry led to, marked until you type or
 * move the caret yourself. A still mark, so it never pulls the eye again.
 */
export const mentionTarget = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update: (marked, transaction) => {
    for (const effect of transaction.effects) {
      if (!effect.is(showMentionTarget)) continue;
      if (effect.value === null) return Decoration.none;
      const line = transaction.state.doc.lineAt(effect.value);
      return Decoration.set([targetLine.range(line.from)]);
    }
    if (transaction.isUserEvent("input") || transaction.isUserEvent("delete") || transaction.isUserEvent("select")) {
      return Decoration.none;
    }
    return marked.map(transaction.changes);
  },
  provide: (field) => EditorView.decorations.from(field),
});
