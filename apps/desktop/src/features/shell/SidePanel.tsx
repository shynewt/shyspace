import { motion, useIsPresent } from "motion/react";
import { type ReactNode, type RefObject, useEffect, useRef, useState } from "react";
import { cx, instantAfterAKey, springs, tweens } from "../../ui";
import { useShellLayout } from "./ShellLayout";

/**
 * A panel at the right of a view (a thread), inside a `relative` parent:
 * docked beside the content while it keeps `--width-channel-min`, otherwise
 * sliding over it with a shadow and no scrim, so the content stays readable
 * at the edge. While it's over the content, Esc closes it, and so does a
 * pointer-down outside it (`onClickAway`), with no scrim, so the click still
 * lands where it was aimed. Mount it inside `AnimatePresence`; closing hands
 * focus back to what opened it, except after a click away, which put focus
 * where it wanted.
 */
export function SidePanel({
  label,
  onClose,
  onClickAway,
  returnFocusTo,
  children,
}: {
  label: string;
  onClose: () => void;
  onClickAway: () => void;
  /** Where focus goes on closing when what opened the panel is gone. */
  returnFocusTo: RefObject<HTMLElement | null>;
  children: ReactNode;
}) {
  const { canDockSidePanel } = useShellLayout();
  const isOverlay = !canDockSidePanel;
  const panel = useRef<HTMLElement>(null);
  const isPresent = useIsPresent();
  // Read while rendering, before the panel's own fields take focus as they mount.
  const [opener] = useState(() => document.activeElement);
  const latestOnClose = useRef(onClose);
  const latestOnClickAway = useRef(onClickAway);
  useEffect(() => {
    latestOnClose.current = onClose;
    latestOnClickAway.current = onClickAway;
  });
  // Set by pointer-downs in the panel's React tree, which includes its menus and popovers in portals.
  const isPointerDownInside = useRef(false);
  const wasClickedAway = useRef(false);

  // On the document, so the channel's own Esc (on the window) sees it was used.
  useEffect(() => {
    if (!isOverlay || !isPresent) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      latestOnClose.current();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [isOverlay, isPresent]);

  // Radix's check for an outside click: the panel's capture handler runs before this listener on the document.
  useEffect(() => {
    if (!isOverlay || !isPresent) return;
    const closeOnPointerDownOutside = (event: PointerEvent) => {
      if (isPointerDownInside.current) {
        isPointerDownInside.current = false;
        return;
      }
      // A dialog over everything, like a profile opened from the panel, isn't the content beneath.
      if (event.target instanceof Element && event.target.closest('[role="dialog"], [role="alertdialog"]')) return;
      wasClickedAway.current = true;
      latestOnClickAway.current();
    };
    document.addEventListener("pointerdown", closeOnPointerDownOutside);
    return () => document.removeEventListener("pointerdown", closeOnPointerDownOutside);
  }, [isOverlay, isPresent]);

  useEffect(() => {
    if (isPresent || wasClickedAway.current) return;
    const focused = document.activeElement;
    const isFocusLeaving = !focused || focused === document.body || panel.current?.contains(focused);
    if (!isFocusLeaving) return;
    const target = opener instanceof HTMLElement && opener.isConnected ? opener : returnFocusTo.current;
    target?.focus({ preventScroll: true });
  }, [isPresent, opener, returnFocusTo]);

  return (
    <motion.aside
      ref={panel}
      aria-label={label}
      onPointerDownCapture={() => {
        isPointerDownInside.current = true;
      }}
      {...(isOverlay ? slideOver() : appearDocked)}
      className={cx(
        "flex w-side-panel max-w-full shrink-0 flex-col border-l border-line bg-surface",
        isOverlay && "absolute inset-y-0 right-0 z-20 shadow-float",
      )}
    >
      {children}
    </motion.aside>
  );
}

/** Over the content, the panel slides in from the right edge, and away faster. */
function slideOver() {
  return {
    initial: { x: "100%" },
    animate: { x: 0, transition: instantAfterAKey(springs.move) },
    exit: { x: "100%", transition: instantAfterAKey(tweens.fast) },
  };
}

/** Docked, the content reflows to make room anyway, so the panel only fades in and goes at once. */
const appearDocked = {
  initial: { opacity: 0 },
  animate: { opacity: 1, transition: tweens.fast },
  exit: { opacity: 0, transition: tweens.instant },
};
