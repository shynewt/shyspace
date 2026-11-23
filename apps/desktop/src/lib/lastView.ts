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

/** One saved view per workspace, under this prefix and the workspace's id. */
const KEY_PREFIX = "shyspace.lastView.";
let hasReportedFailure = false;

/** Opens the next launch at the first channel, as after a screen failed to draw there. Every workspace's, since the one that failed isn't known then. */
export function forgetLastView(): void {
  try {
    for (const key of Object.keys(localStorage)) if (key.startsWith(KEY_PREFIX)) localStorage.removeItem(key);
  } catch (error) {
    reportError(error);
  }
}

/** The last view `me` saved in `workspaceId`, or null if there's none or it can't be read. */
export function readLastView(workspaceId: string, me: string): LastView | null {
  // Unreadable storage only means opening at the first channel.
  try {
    const stored = localStorage.getItem(KEY_PREFIX + workspaceId);
    return stored ? parseLastView(JSON.parse(stored), me) : null;
  } catch {
    return null;
  }
}

export function saveLastView(workspaceId: string, me: string, { view, anchor }: LastView): void {
  try {
    localStorage.setItem(KEY_PREFIX + workspaceId, JSON.stringify({ me, view: withoutFocus(view), anchor }));
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
  if (value.kind === "inbox" || value.kind === "later" || value.kind === "files" || value.kind === "drafts") {
    return { kind: value.kind };
  }
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
