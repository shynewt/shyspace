import { useEffect, useRef, useState } from "react";
import { type Change, onChange } from "./api";
import { reportError } from "./toast";

/**
 * Loads data now and again whenever a relevant change arrives. Returns
 * undefined until the first load finishes. `load` must be stable (a module
 * function or a useCallback): a new `load` starts over, like a new query.
 */
export function useLive<T>(load: () => Promise<T>, isRelevant: (change: Change) => boolean) {
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
    const unsubscribe = onChange((change) => {
      if (relevantRef.current(change)) refresh();
    });
    return () => {
      isCancelled = true;
      unsubscribe();
    };
  }, [load]);

  return value;
}
