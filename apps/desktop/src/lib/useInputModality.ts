import { useEffect } from "react";

const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "Meta", "AltGraph", "CapsLock"]);

/**
 * Marks <html> with how the app was last used: `data-modality="pointer"` or
 * `"keyboard"`. Focus that a script moves after a click (Radix handing focus
 * back to a menu's trigger) still matches :focus-visible when it came from a
 * text field, so the stylesheet uses this to keep focus rings for keyboard use.
 */
export function useInputModality() {
  useEffect(() => {
    const root = document.documentElement;
    const rememberPointer = () => {
      root.dataset.modality = "pointer";
    };
    const rememberKeyboard = (event: KeyboardEvent) => {
      if (!MODIFIER_KEYS.has(event.key)) root.dataset.modality = "keyboard";
    };
    document.addEventListener("pointerdown", rememberPointer, true);
    document.addEventListener("keydown", rememberKeyboard, true);
    return () => {
      document.removeEventListener("pointerdown", rememberPointer, true);
      document.removeEventListener("keydown", rememberKeyboard, true);
    };
  }, []);
}
