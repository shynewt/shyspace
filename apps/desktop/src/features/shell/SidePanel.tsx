import { motion, useIsPresent } from "motion/react";
import { type ReactNode, type RefObject, useEffect, useRef, useState } from "react";
import { cx, springs } from "../../ui";
import { useShellLayout } from "./ShellLayout";

/**
 * A panel at the right of a view (a thread), inside a `relative` parent:
 * docked beside the content while it keeps `--width-channel-min`, otherwise
 * sliding over it with a shadow and no scrim, so the content stays readable
 * at the edge. Esc closes it while it's over the content. Mount it inside
 * `AnimatePresence`; closing hands focus back to what opened it.
 */
export function SidePanel({
  label,
  onClose,
  returnFocusTo,
  children,
}: {
  label: string;
  onClose: () => void;
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
  useEffect(() => {
    latestOnClose.current = onClose;
  });

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

  useEffect(() => {
    if (isPresent) return;
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
      initial={isOverlay ? { x: "100%" } : { opacity: 0, x: 24 }}
      animate={{ opacity: 1, x: 0 }}
      exit={isOverlay ? { x: "100%" } : { opacity: 0, x: 24 }}
      transition={springs.smooth}
      className={cx(
        "flex w-side-panel max-w-full shrink-0 flex-col border-l border-line bg-surface",
        isOverlay && "absolute inset-y-0 right-0 z-20 shadow-float",
      )}
    >
      {children}
    </motion.aside>
  );
}
