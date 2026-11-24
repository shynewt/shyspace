import { useSyncExternalStore } from "react";

export type Toast = {
  id: number;
  text: string;
  /** "success" confirms something you did; "info" tells you something; errors are announced as alerts. */
  tone: "success" | "info" | "error" | "reminder";
  /** A button beside the text. */
  action?: { kind: "undo" | "open"; run: () => Promise<void> | void };
};

const TOAST_MS = 2600;
const ERROR_TOAST_MS = 6000;
const UNDO_TOAST_MS = 5000;
const REMINDER_TOAST_MS = 15000;

let toasts: Toast[] = [];
let nextId = 1;
const subscribers = new Set<() => void>();
function publish() {
  for (const notify of subscribers) notify();
}

function show(toast: Omit<Toast, "id">, durationMs: number) {
  const id = nextId++;
  toasts = [...toasts, { id, ...toast }];
  publish();
  setTimeout(() => dismissToast(id), durationMs);
}

export function showToast(text: string, tone: "success" | "info" = "success") {
  show({ text, tone }, TOAST_MS);
}

/** A confirmation with an Undo button, shown long enough to reach it. */
export function showUndoToast(text: string, undo: () => Promise<void>) {
  show({ text, tone: "success", action: { kind: "undo", run: undo } }, UNDO_TOAST_MS);
}

/**
 * Takes away every Undo still on screen. Undo acts on the selected workspace,
 * so once another is shown it would undo there instead.
 */
export function dismissUndoToasts() {
  const kept = toasts.filter((toast) => toast.action?.kind !== "undo");
  if (kept.length === toasts.length) return;
  toasts = kept;
  publish();
}

/** A reminder that came due, with a button to open what was saved. */
export function showReminderToast(text: string, open: () => void) {
  show({ text, tone: "reminder", action: { kind: "open", run: open } }, REMINDER_TOAST_MS);
}

export function reportError(error: unknown) {
  console.error(error);
  show({ text: errorMessage(error), tone: "error" }, ERROR_TOAST_MS);
}

/** The core reports errors as strings; anything else is a bug in the app. */
export function errorMessage(error: unknown): string {
  return typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
}

export function dismissToast(id: number) {
  toasts = toasts.filter((toast) => toast.id !== id);
  publish();
}

export function useToasts() {
  return useSyncExternalStore(
    (notify) => {
      subscribers.add(notify);
      return () => subscribers.delete(notify);
    },
    () => toasts,
  );
}
