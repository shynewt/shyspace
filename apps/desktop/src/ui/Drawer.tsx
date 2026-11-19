import { X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { Dialog as RadixDialog } from "radix-ui";
import { type ReactNode, useRef } from "react";
import { IconButton } from "./IconButton";
import { fade, springs } from "./motion";

/** A panel sliding in from the right edge, modal like a dialog. */
export function Drawer({
  open,
  onOpenChange,
  title,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  children: ReactNode;
}) {
  const panel = useRef<HTMLElement>(null);
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <AnimatePresence>
        {open && (
          <RadixDialog.Portal forceMount>
            <RadixDialog.Overlay asChild>
              <motion.div {...fade} className="fixed inset-0 z-30 bg-black/20" />
            </RadixDialog.Overlay>
            {/* Focus the panel, not its first button: a focused Close button
                shows its tooltip, which would take the first Escape. */}
            <RadixDialog.Content
              asChild
              onOpenAutoFocus={(event) => {
                event.preventDefault();
                panel.current?.focus();
              }}
            >
              <motion.aside
                ref={panel}
                initial={{ x: "100%" }}
                animate={{ x: 0 }}
                exit={{ x: "100%", transition: { duration: 0.18, ease: "easeIn" } }}
                transition={springs.smooth}
                className="fixed inset-y-0 right-0 z-30 flex w-[400px] max-w-[92vw] flex-col border-l border-line bg-surface shadow-float outline-none"
              >
                <header className="flex h-13 shrink-0 items-center justify-between border-b border-line px-5">
                  <RadixDialog.Title className="font-semibold">{title}</RadixDialog.Title>
                  <RadixDialog.Description className="sr-only">{title}</RadixDialog.Description>
                  <RadixDialog.Close asChild>
                    <IconButton label="Close" icon={<X size={16} />} />
                  </RadixDialog.Close>
                </header>
                <div className="flex-1 overflow-y-auto p-3">{children}</div>
              </motion.aside>
            </RadixDialog.Content>
          </RadixDialog.Portal>
        )}
      </AnimatePresence>
    </RadixDialog.Root>
  );
}
