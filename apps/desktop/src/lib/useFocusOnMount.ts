import { type RefObject, useEffect } from "react";

/**
 * Focuses an element when it appears. Only for elements that appear because
 * the user just asked for them (a new-channel field, a composer they opened).
 */
export function useFocusOnMount(ref: RefObject<HTMLElement | null>) {
  useEffect(() => {
    ref.current?.focus();
  }, [ref]);
}
