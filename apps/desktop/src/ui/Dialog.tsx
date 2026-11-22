import { X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { Dialog as RadixDialog } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "./cx";
import { IconButton } from "./IconButton";
import { fade, pop } from "./motion";

type DialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Announced to screen readers; shown unless `hideTitle`. */
  title: string;
  description?: ReactNode;
  hideTitle?: boolean;
  /** Where the dialog sits: centered, or near the top like a command palette. */
  placement?: "center" | "top";
  width?: number;
  /** Shows a close button in the top corner, for dialogs without their own Cancel or Done. */
  hasCloseButton?: boolean;
  /** "none" for content that lays itself out edge to edge, like the search palette. */
  padding?: "standard" | "none";
  /** Where focus goes when it closes, when what opened it is gone (like a card). */
  returnFocusTo?: HTMLElement | null;
  /** "instant" for a dialog opened many times a day, like the command palette. */
  entrance?: "pop" | "instant";
  /**
   * "dialog" focuses the dialog itself on opening, for viewers whose first
   * control is an icon button: its tooltip would take the first Escape.
   */
  initialFocus?: "first" | "dialog";
  children: ReactNode;
};

/**
 * A modal surface. Radix handles focus trapping, Escape and outside clicks.
 * It never grows taller than the window: longer content scrolls inside.
 */
export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  hideTitle = false,
  placement = "center",
  width = 440,
  hasCloseButton = false,
  padding = "standard",
  returnFocusTo,
  entrance = "pop",
  initialFocus = "first",
  children,
}: DialogProps) {
  const isInstant = entrance === "instant";
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <AnimatePresence>
        {open && (
          <RadixDialog.Portal forceMount>
            <RadixDialog.Overlay asChild>
              <motion.div {...(isInstant ? {} : fade)} className="fixed inset-0 z-40 bg-scrim" />
            </RadixDialog.Overlay>
            <div
              className={cx(
                "pointer-events-none fixed inset-0 z-40 flex justify-center px-4",
                placement === "top" ? "items-start pt-[14vh]" : "items-center",
              )}
            >
              <RadixDialog.Content
                asChild
                onOpenAutoFocus={(event) => {
                  if (initialFocus !== "dialog") return;
                  event.preventDefault();
                  if (event.currentTarget instanceof HTMLElement) event.currentTarget.focus();
                }}
                onCloseAutoFocus={(event) => {
                  if (!returnFocusTo?.isConnected) return;
                  event.preventDefault();
                  returnFocusTo.focus();
                }}
              >
                <motion.div
                  {...(isInstant ? {} : pop)}
                  style={{ width }}
                  className={cx(
                    "pointer-events-auto relative max-w-full overflow-y-auto rounded-2xl border border-line bg-surface shadow-float outline-none",
                    placement === "top" ? "max-h-[calc(86dvh-1rem)]" : "max-h-[calc(100dvh-2rem)]",
                  )}
                >
                  <RadixDialog.Title className={hideTitle ? "sr-only" : "px-5 pt-5 text-lg font-semibold"}>
                    {title}
                  </RadixDialog.Title>
                  {description ? (
                    <RadixDialog.Description className="px-5 pt-1 text-sm leading-relaxed text-muted">
                      {description}
                    </RadixDialog.Description>
                  ) : (
                    <RadixDialog.Description className="sr-only">{title}</RadixDialog.Description>
                  )}
                  {padding === "standard" ? (
                    <div className="flex flex-col gap-4 px-5 pt-4 pb-5">{children}</div>
                  ) : (
                    children
                  )}
                  {/* After the content, so opening the dialog focuses the content
                      rather than this button (whose tooltip would take the first Escape). */}
                  {hasCloseButton && (
                    <RadixDialog.Close asChild>
                      <IconButton label="Close" icon={<X size={16} />} className="absolute top-3.5 right-3.5" />
                    </RadixDialog.Close>
                  )}
                </motion.div>
              </RadixDialog.Content>
            </div>
          </RadixDialog.Portal>
        )}
      </AnimatePresence>
    </RadixDialog.Root>
  );
}

/** The buttons at the foot of a dialog, the main one last. */
export function DialogActions({ children }: { children: ReactNode }) {
  return <div className="flex justify-end gap-2">{children}</div>;
}
