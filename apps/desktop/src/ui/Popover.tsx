import { motion } from "motion/react";
import { Popover as RadixPopover } from "radix-ui";
import type { ReactNode, RefObject } from "react";
import { pop, springs } from "./motion";

/** A small floating surface anchored to its trigger, like an emoji picker. */
export function Popover({
  trigger,
  children,
  open,
  onOpenChange,
  side = "top",
  align = "end",
  returnFocusTo,
}: {
  trigger: ReactNode;
  children: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  side?: "top" | "bottom" | "left" | "right";
  align?: "start" | "center" | "end";
  /** Where focus goes when it closes, instead of back to the trigger. */
  returnFocusTo?: RefObject<HTMLElement | null>;
}) {
  return (
    <RadixPopover.Root open={open} onOpenChange={onOpenChange}>
      <RadixPopover.Trigger asChild>{trigger}</RadixPopover.Trigger>
      <RadixPopover.Portal>
        <RadixPopover.Content
          side={side}
          align={align}
          sideOffset={6}
          asChild
          onCloseAutoFocus={(event) => {
            if (!returnFocusTo) return;
            event.preventDefault();
            returnFocusTo.current?.focus();
          }}
        >
          <motion.div
            {...pop}
            transition={springs.snappy}
            className="z-50 rounded-xl border border-line bg-surface p-1 shadow-float outline-none"
          >
            {children}
          </motion.div>
        </RadixPopover.Content>
      </RadixPopover.Portal>
    </RadixPopover.Root>
  );
}
