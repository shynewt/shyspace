import { AnimatePresence, type HTMLMotionProps, motion } from "motion/react";
import { Tooltip as RadixTooltip } from "radix-ui";
import { type ReactNode, useState } from "react";
import { pop } from "./motion";

/** Wrap the app once so tooltips share timing. */
export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <RadixTooltip.Provider delayDuration={450} skipDelayDuration={200}>
      {children}
    </RadixTooltip.Provider>
  );
}

export function Tooltip({
  label,
  side = "top",
  shouldShow,
  children,
}: {
  label: ReactNode;
  side?: "top" | "bottom" | "left" | "right";
  /** Asked as it's about to show; false keeps it hidden, e.g. while the text it repeats is on screen. */
  shouldShow?: () => boolean;
  children: ReactNode;
}) {
  const [isOpen, setOpen] = useState(false);
  return (
    <RadixTooltip.Root open={isOpen} onOpenChange={(open) => setOpen(open && (shouldShow?.() ?? true))}>
      <RadixTooltip.Trigger asChild>{children}</RadixTooltip.Trigger>
      <AnimatePresence>
        {isOpen && (
          <RadixTooltip.Portal forceMount>
            <RadixTooltip.Content side={side} sideOffset={6} asChild>
              <TooltipSurface>{label}</TooltipSurface>
            </RadixTooltip.Content>
          </RadixTooltip.Portal>
        )}
      </AnimatePresence>
    </RadixTooltip.Root>
  );
}

/**
 * Pops in after the hover delay; a tooltip opened at once (Radix's
 * `instant-open`: moving on to a neighbour, or by focus) just appears.
 */
function TooltipSurface({ "data-state": state, ...props }: HTMLMotionProps<"div"> & { "data-state"?: string }) {
  return (
    <motion.div
      {...props}
      {...pop}
      data-state={state}
      initial={state === "instant-open" ? false : pop.initial}
      className="z-50 origin-(--radix-tooltip-content-transform-origin) max-w-tooltip rounded-md bg-ink px-2 py-1 text-xs font-medium wrap-anywhere text-bg shadow-float"
    />
  );
}
