import { useSyncExternalStore } from "react";

export type Toast = {
  id: number;
  text: string;
  tone: "error" | "info";
  /** Offers an Undo button that runs this. */
  undo?: () => Promise<void>;
};

const UNDO_TOAST_MS = 5000;

let toasts: Toast[] = [];
let nextId = 1;
const subscribers = new Set<() => void>();
function publish() {
  for (const notify of subscribers) notify();
}

export function showToast(text: string, tone: Toast["tone"] = "info") {
  const toast = { id: nextId++, text, tone };
  toasts = [...toasts, toast];
  publish();
  setTimeout(() => dismissToast(toast.id), tone === "error" ? 6000 : 2600);
}

/** A confirmation with an Undo button, shown long enough to reach it. */
export function showUndoToast(text: string, undo: () => Promise<void>) {
  const toast: Toast = { id: nextId++, text, tone: "info", undo };
  toasts = [...toasts, toast];
  publish();
  setTimeout(() => dismissToast(toast.id), UNDO_TOAST_MS);
}

export function reportError(error: unknown) {
  console.error(error);
  showToast(typeof error === "string" ? error : error instanceof Error ? error.message : String(error), "error");
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
