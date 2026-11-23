import { useEffect, useRef, useState } from "react";
import { type Change, onAnyWorkspaceChange, onChange } from "./api";
import { reportError } from "./toast";

/**
 * Loads data now and again whenever a relevant change arrives. Returns
 * undefined until the first load finishes. `load` must be stable (a module
 * function or a useCallback): a new `load` starts over, like a new query.
 */
export function useLive<T>(load: () => Promise<T>, isRelevant: (change: Change) => boolean) {
  return useLiveFrom(onChange, load, isRelevant);
}

/** The same, for data that spans workspaces (your call, a ring): reloaded on changes from any of them. */
export function useLiveEverywhere<T>(load: () => Promise<T>, isRelevant: (change: Change) => boolean) {
  return useLiveFrom(subscribeEverywhere, load, isRelevant);
}

function subscribeEverywhere(listener: (change: Change) => void): () => void {
  return onAnyWorkspaceChange((_, change) => listener(change));
}

function useLiveFrom<T>(
  subscribe: (listener: (change: Change) => void) => () => void,
  load: () => Promise<T>,
  isRelevant: (change: Change) => boolean,
) {
  const [value, setValue] = useState<T>();
  // Only consulted when a change arrives, so the latest one is always right.
  const relevantRef = useRef(isRelevant);
  relevantRef.current = isRelevant;

  useEffect(() => {
    let isCancelled = false;
    let generation = 0;
    const refresh = () => {
      const current = ++generation;
      load().then(
        (next) => {
          if (!isCancelled && current === generation) setValue(next);
        },
        (error) => {
          if (!isCancelled) reportError(error);
        },
      );
    };
    refresh();
    const unsubscribe = subscribe((change) => {
      if (relevantRef.current(change)) refresh();
    });
    return () => {
      isCancelled = true;
      unsubscribe();
    };
  }, [subscribe, load]);

  return value;
}
