import { useSyncExternalStore } from "react";

const EVENTS = ["focus", "blur"] as const;

function subscribe(notify: () => void): () => void {
  for (const event of EVENTS) window.addEventListener(event, notify);
  document.addEventListener("visibilitychange", notify);
  return () => {
    for (const event of EVENTS) window.removeEventListener(event, notify);
    document.removeEventListener("visibilitychange", notify);
  };
}

function isAttentive(): boolean {
  return document.hasFocus() && document.visibilityState === "visible";
}

/** Whether you could be looking: the window has focus and isn't hidden or minimised. */
export function useWindowAttention(): boolean {
  return useSyncExternalStore(subscribe, isAttentive);
}
