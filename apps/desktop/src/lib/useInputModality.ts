import { useEffect } from "react";
import { hasCommandModifier } from "./shortcuts";

/**
 * Keys that move focus or use the focused control, alone or with Shift. Others
 * (typing, Esc, shortcuts) leave the modality as it was.
 */
const KEYBOARD_USE_KEYS = new Set([
  "Tab",
  "Enter",
  " ",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);

/**
 * How long after the window gains focus keys don't count: a Wayland compositor
 * hands the window the keys still held from switching to it (Super+arrow).
 */
const WINDOW_FOCUS_GRACE_MS = 150;

/**
 * Marks <html> with how the app was last used: `data-modality="pointer"` or
 * `"keyboard"`. Focus that a script moves after a click (Radix handing focus
 * back to a menu's trigger) still matches :focus-visible when it came from a
 * text field, and browsers show a ring on a clicked button once any key is
 * pressed, so the stylesheet uses this to keep focus rings for moving with
 * the keyboard.
 */
export function useInputModality() {
  useEffect(() => {
    const root = document.documentElement;
    const rememberPointer = () => {
      root.dataset.modality = "pointer";
    };
    let windowFocusedAt = Number.NEGATIVE_INFINITY;
    const keysHeldAtWindowFocus = new Set<string>();
    const rememberWindowFocus = () => {
      windowFocusedAt = performance.now();
    };
    const rememberKeyboard = (event: KeyboardEvent) => {
      if (performance.now() - windowFocusedAt < WINDOW_FOCUS_GRACE_MS) {
        keysHeldAtWindowFocus.add(event.code);
        return;
      }
      if (event.repeat && keysHeldAtWindowFocus.has(event.code)) return;
      if (KEYBOARD_USE_KEYS.has(event.key) && !hasCommandModifier(event)) root.dataset.modality = "keyboard";
    };
    const forgetReleasedKey = (event: KeyboardEvent) => {
      keysHeldAtWindowFocus.delete(event.code);
    };
    document.addEventListener("pointerdown", rememberPointer, true);
    document.addEventListener("keydown", rememberKeyboard, true);
    document.addEventListener("keyup", forgetReleasedKey, true);
    window.addEventListener("focus", rememberWindowFocus);
    return () => {
      document.removeEventListener("pointerdown", rememberPointer, true);
      document.removeEventListener("keydown", rememberKeyboard, true);
      document.removeEventListener("keyup", forgetReleasedKey, true);
      window.removeEventListener("focus", rememberWindowFocus);
    };
  }, []);
}
