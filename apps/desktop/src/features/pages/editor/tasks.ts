import type { EditorView } from "@codemirror/view";

const TASK_PREFIX = /^(\s*(?:>\s?)*\s*[-*+]\s+)\[([ xX])\]/;

/**
 * Checks or unchecks the checklist item on the line holding `position`.
 * Positions come from the document as it is now, never remembered ones, so
 * this stays right while others edit. Returns whether the line was a task.
 */
export function toggleTaskAt(view: EditorView, position: number): boolean {
  const line = view.state.doc.lineAt(position);
  const match = TASK_PREFIX.exec(line.text);
  if (!match) return false;
  const marker = line.from + match[1].length + 1;
  view.dispatch({
    changes: { from: marker, to: marker + 1, insert: match[2] === " " ? "x" : " " },
    userEvent: "input.toggle",
  });
  return true;
}
