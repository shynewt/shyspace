import { type RefObject, useEffect } from "react";

/** Whether the user has selected text inside `element`. */
function hasSelectionInside(element: Element): boolean {
  const selection = window.getSelection();
  return Boolean(selection && !selection.isCollapsed && selection.anchorNode && element.contains(selection.anchorNode));
}

/**
 * Opens a custom menu on right-click over `element`. With text selected
 * inside it, the system menu (with Copy) shows instead.
 */
export function useContextMenu(ref: RefObject<HTMLElement | null>, onOpen: () => void) {
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const open = (event: MouseEvent) => {
      if (hasSelectionInside(element)) return;
      event.preventDefault();
      onOpen();
    };
    element.addEventListener("contextmenu", open);
    return () => element.removeEventListener("contextmenu", open);
  }, [ref, onOpen]);
}

/**
 * The browser's own menu (Back, Reload, Inspect Element) means nothing in the
 * app; keep it only where it helps: in text fields and over selected text.
 */
export function useNoBrowserMenu() {
  useEffect(() => {
    const hideBrowserMenu = (event: MouseEvent) => {
      if (event.defaultPrevented) return;
      const target = event.target instanceof Element ? event.target : null;
      const isEditable = target?.closest("input, textarea, [contenteditable='true']");
      const selection = window.getSelection();
      if (isEditable || (selection && !selection.isCollapsed)) return;
      event.preventDefault();
    };
    window.addEventListener("contextmenu", hideBrowserMenu);
    return () => window.removeEventListener("contextmenu", hideBrowserMenu);
  }, []);
}
