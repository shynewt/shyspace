import { AnimatePresence, motion } from "motion/react";
import { Dialog as RadixDialog } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "./cx";
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
  children: ReactNode;
};

/** A modal surface. Radix handles focus trapping, Escape and outside clicks. */
export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  hideTitle = false,
  placement = "center",
  width = 440,
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
                  className="pointer-events-auto max-w-full overflow-hidden rounded-2xl border border-line bg-surface shadow-float outline-none"
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
                </motion.div>
              </RadixDialog.Content>
            </div>
          </RadixDialog.Portal>
        )}
      </AnimatePresence>
    </RadixDialog.Root>
  );
}
