import { X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { Dialog as RadixDialog } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "./cx";
import { IconButton } from "./IconButton";
import { fade, pop, springs } from "./motion";

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
  children,
}: DialogProps) {
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <AnimatePresence>
        {open && (
          <RadixDialog.Portal forceMount>
            <RadixDialog.Overlay asChild>
              <motion.div {...fade} className="fixed inset-0 z-40 bg-black/30 backdrop-blur-[2px]" />
            </RadixDialog.Overlay>
            <div
              className={cx(
                "pointer-events-none fixed inset-0 z-40 flex justify-center px-4",
                placement === "top" ? "items-start pt-[14vh]" : "items-center",
              )}
            >
              <RadixDialog.Content asChild>
                <motion.div
                  {...pop}
                  transition={springs.smooth}
                  style={{ width }}
                  className={cx(
                    "pointer-events-auto relative max-w-full overflow-y-auto rounded-2xl border border-line bg-surface shadow-float outline-none",
                    placement === "top" ? "max-h-[calc(86dvh-1rem)]" : "max-h-[calc(100dvh-2rem)]",
                  )}
                >
                  <RadixDialog.Title className={hideTitle ? "sr-only" : "px-5 pt-5 text-[15px] font-semibold"}>
                    {title}
                  </RadixDialog.Title>
                  {description ? (
                    <RadixDialog.Description className="px-5 pt-1 text-[13px] leading-relaxed text-muted">
                      {description}
                    </RadixDialog.Description>
                  ) : (
                    <RadixDialog.Description className="sr-only">{title}</RadixDialog.Description>
                  )}
                  {children}
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
