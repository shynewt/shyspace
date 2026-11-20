/**
 * Back and forward through the places you've been, kept in memory. Each
 * entry holds a view (with its open thread) and where to scroll it to. Pure,
 * so the rules are easy to test; `useNavigation` wires it to the app.
 */
import type { ScrollAnchor } from "./scrollAnchor";
import type { View } from "./workspace";

export type HistoryEntry = {
  view: View;
  /** Where to scroll the view when it's shown; null opens it as usual (unread line, or the newest message). */
  anchor: ScrollAnchor | null;
};

export type History = { entries: HistoryEntry[]; index: number };

/** "all-skipped": there were places that way, but none can be opened any more. */
export type StepOutcome = "moved" | "none" | "all-skipped";

export const MAX_HISTORY_ENTRIES = 50;

export function startHistory(view: View, anchor: ScrollAnchor | null = null): History {
  return { entries: [{ view, anchor }], index: 0 };
}

export function currentEntry(history: History): HistoryEntry {
  return history.entries[history.index];
}

/** The same place: opening it again, without jumping to a message, adds nothing. */
function isSamePlace(current: View, next: View): boolean {
  if (next.kind === "channel" && next.focus) return false;
  if (current.kind === "channel" && next.kind === "channel") {
    return current.id === next.id && current.thread === next.thread;
  }
  if (current.kind === "page" && next.kind === "page") return current.id === next.id;
  return current.kind === next.kind;
}

/** Opens `view` as a new place after the current one; anything ahead of it is dropped. */
export function visit(history: History, view: View, anchor: ScrollAnchor | null = null): History {
  if (isSamePlace(currentEntry(history).view, view)) return history;
  const entries = [...history.entries.slice(0, history.index + 1), { view, anchor }].slice(-MAX_HISTORY_ENTRIES);
  return { entries, index: entries.length - 1 };
}

/**
 * Records where the current place was scrolled as you leave it. Coming back
 * returns there, so a message it once jumped to no longer matters.
 */
export function leaveWithAnchor(history: History, anchor: ScrollAnchor | null): History {
  return withCurrent(history, { view: withoutFocus(currentEntry(history).view), anchor });
}

/** Swaps the current place for another without adding one, as when it was deleted or its thread is gone. */
export function replaceCurrent(history: History, view: View, anchor: ScrollAnchor | null = null): History {
  return withCurrent(history, { view, anchor });
}

/** The view without a message to jump to: where it's scrolled is kept separately. */
export function withoutFocus(view: View): View {
  return view.kind === "channel" ? { kind: "channel", id: view.id, thread: view.thread } : view;
}

function withCurrent(history: History, entry: HistoryEntry): History {
  return {
    ...history,
    entries: history.entries.map((existing, index) => (index === history.index ? entry : existing)),
  };
}

/**
 * Moves one place back (-1) or forward (1), skipping places that can't be
 * opened any more. Those are forgotten wherever they are in the history.
 */
export function step(
  history: History,
  direction: -1 | 1,
  isAvailable: (view: View) => boolean,
): { history: History; outcome: StepOutcome } {
  const kept = history.entries.filter((entry, index) => index === history.index || isAvailable(entry.view));
  const index = kept.indexOf(currentEntry(history));
  const target = index + direction;
  if (target >= 0 && target < kept.length) return { history: { entries: kept, index: target }, outcome: "moved" };
  if (kept.length === history.entries.length) return { history, outcome: "none" };
  const hadPlacesThatWay = direction === -1 ? history.index > 0 : history.index < history.entries.length - 1;
  return { history: { entries: kept, index }, outcome: hadPlacesThatWay ? "all-skipped" : "none" };
}
