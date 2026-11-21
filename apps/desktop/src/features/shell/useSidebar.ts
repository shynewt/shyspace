import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useIsWindowAtLeast } from "../../lib/layout";
import { usePreferences } from "../../lib/preferences";

/** "left": closed because you chose somewhere to go, so focus stays with where you went. */
type DrawerState = "open" | "closed" | "left";

export type SidebarState = {
  /** Docked beside the content in a wide window; a drawer over it in a narrow one. */
  mode: "docked" | "drawer";
  /** Docked and not hidden: the content starts beside it. */
  isDocked: boolean;
  drawer: DrawerState;
  /** Hides or shows the docked sidebar (remembered), or opens or closes the drawer. */
  toggle: () => void;
  closeDrawer: () => void;
  /** Closes the drawer because you chose somewhere to go (even where you already are). */
  leaveDrawer: () => void;
  /** For a toggle button that just appeared: whether it takes focus from the one that went away. */
  claimToggleFocus: () => boolean;
};

export function useSidebar(): SidebarState {
  const isWide = useIsWindowAtLeast("--breakpoint-sidebar-docks");
  const [{ isSidebarHidden }, setPreferences] = usePreferences();
  const [drawer, setDrawer] = useState<DrawerState>("closed");
  // A drawer left open docks as the window grows, and is closed if it shrinks again.
  if (isWide && drawer === "open") setDrawer("closed");
  const isFocusMoving = useRef(false);
  // After each change, a toggle that appeared has claimed focus (children's effects run first); nobody else may.
  useEffect(() => {
    isFocusMoving.current = false;
  });

  const toggle = useCallback(() => {
    if (!isWide) {
      setDrawer((current) => (current === "open" ? "closed" : "open"));
      return;
    }
    // The button pressed (or anything focused in the sidebar) goes away; the other toggle takes over.
    const focused = document.activeElement;
    isFocusMoving.current =
      focused instanceof HTMLElement &&
      (focused.closest("[data-sidebar-toggle]") !== null || focused.closest("[data-sidebar]") !== null);
    setPreferences({ isSidebarHidden: !isSidebarHidden });
  }, [isWide, isSidebarHidden, setPreferences]);

  const closeDrawer = useCallback(() => setDrawer("closed"), []);
  const leaveDrawer = useCallback(() => setDrawer((current) => (current === "open" ? "left" : current)), []);

  const claimToggleFocus = useCallback(() => {
    const isClaimed = isFocusMoving.current;
    isFocusMoving.current = false;
    return isClaimed;
  }, []);

  return useMemo(
    () => ({
      mode: isWide ? "docked" : "drawer",
      isDocked: isWide && !isSidebarHidden,
      drawer: isWide ? "closed" : drawer,
      toggle,
      closeDrawer,
      leaveDrawer,
      claimToggleFocus,
    }),
    [isWide, isSidebarHidden, drawer, toggle, closeDrawer, leaveDrawer, claimToggleFocus],
  );
}
