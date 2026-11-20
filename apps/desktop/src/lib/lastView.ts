/**
 * The view you were in (with its thread and scroll position), so the app
 * reopens there. A per-device convenience in the webview's local storage,
 * nothing else from the history is kept.
 */
import { withoutFocus } from "./history";
import type { ScrollAnchor } from "./scrollAnchor";
import { reportError } from "./toast";
import type { View } from "./workspace";

export type LastView = { view: View; anchor: ScrollAnchor | null };

const KEY = "shyspace.lastView";
let hasReportedFailure = false;

/** The last view saved by `me`, or null if there's none or it can't be read. */
export function readLastView(me: string): LastView | null {
  // Unreadable storage only means opening at the first channel.
  try {
    const stored = localStorage.getItem(KEY);
    return stored ? parseLastView(JSON.parse(stored), me) : null;
  } catch {
    return null;
  }
}

export function saveLastView(me: string, { view, anchor }: LastView): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ me, view: withoutFocus(view), anchor }));
  } catch (error) {
    // Saved often (as you scroll); one warning is enough.
    if (hasReportedFailure) return;
    hasReportedFailure = true;
    reportError(new Error(`couldn't remember where you are, so the app opens at the first channel (${String(error)})`));
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parseLastView(stored: unknown, me: string): LastView | null {
  if (!isRecord(stored) || stored.me !== me) return null;
  const view = parseView(stored.view);
  if (!view) return null;
  return { view, anchor: parseAnchor(stored.anchor) };
}

function parseView(value: unknown): View | null {
  if (!isRecord(value)) return null;
  if (value.kind === "inbox" || value.kind === "later") return { kind: value.kind };
  if (typeof value.id !== "string") return null;
  if (value.kind === "page") return { kind: "page", id: value.id };
  if (value.kind !== "channel") return null;
  return { kind: "channel", id: value.id, thread: typeof value.thread === "string" ? value.thread : undefined };
}

function parseAnchor(value: unknown): ScrollAnchor | null {
  if (!isRecord(value) || typeof value.messageId !== "string") return null;
  if (typeof value.offsetPx !== "number" || !Number.isFinite(value.offsetPx)) return null;
  return { messageId: value.messageId, offsetPx: value.offsetPx };
}
