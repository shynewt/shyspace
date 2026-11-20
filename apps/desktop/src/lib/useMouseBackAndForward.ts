import { useEffect } from "react";

const BACK_BUTTON = 3;
const FORWARD_BUTTON = 4;

/**
 * The mouse's back and forward buttons, wherever focus is. The action runs
 * on release; the press and its click are swallowed too, so the webview
 * never tries a page navigation of its own.
 */
export function useMouseBackAndForward(back: () => void, forward: () => void) {
  useEffect(() => {
    const isNavigationButton = (event: MouseEvent) => event.button === BACK_BUTTON || event.button === FORWARD_BUTTON;
    const swallow = (event: MouseEvent) => {
      if (isNavigationButton(event)) event.preventDefault();
    };
    const onRelease = (event: MouseEvent) => {
      if (!isNavigationButton(event)) return;
      event.preventDefault();
      if (event.button === BACK_BUTTON) back();
      else forward();
    };
    window.addEventListener("mousedown", swallow);
    window.addEventListener("auxclick", swallow);
    window.addEventListener("mouseup", onRelease);
    return () => {
      window.removeEventListener("mousedown", swallow);
      window.removeEventListener("auxclick", swallow);
      window.removeEventListener("mouseup", onRelease);
    };
  }, [back, forward]);
}
