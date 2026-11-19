import { useEffect, useRef, useState } from "react";
import { type Change, onChange } from "./api";
import { reportError } from "./toast";

/**
 * Loads data now and again whenever a relevant change arrives. Returns
 * undefined until the first load finishes. `deps` decides when to start over
 * (like useEffect); the callbacks may change freely between renders.
 */
export function useLive<T>(load: () => Promise<T>, deps: unknown[], isRelevant: (change: Change) => boolean) {
  const [value, setValue] = useState<T>();
  const loadRef = useRef(load);
  const relevantRef = useRef(isRelevant);
  loadRef.current = load;
  relevantRef.current = isRelevant;

  useEffect(() => {
    let cancelled = false;
    let generation = 0;
    const refresh = () => {
      const current = ++generation;
      loadRef.current().then(
        (next) => !cancelled && current === generation && setValue(next),
        (error) => !cancelled && reportError(error),
      );
    };
    refresh();
    const unsubscribe = onChange((change) => relevantRef.current(change) && refresh());
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, deps);

  return value;
}
