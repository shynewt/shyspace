import { motion } from "motion/react";
import { DropdownMenu } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "./cx";
import { pop, springs } from "./motion";

export type MenuItem = {
  label: string;
  icon?: ReactNode;
  onSelect: () => void;
  isDestructive?: boolean;
};

/** A dropdown of actions. Keyboard navigation comes from Radix. */
export function Menu({ trigger, items }: { trigger: ReactNode; items: MenuItem[] }) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="end" sideOffset={4} asChild>
          <motion.div
            {...pop}
            transition={springs.snappy}
            className="z-50 min-w-[180px] rounded-xl border border-line bg-surface p-1 shadow-float outline-none"
          >
            {items.map((item) => (
              <DropdownMenu.Item
                key={item.label}
                onSelect={item.onSelect}
                className={cx(
                  "flex h-8 cursor-default items-center gap-2 rounded-lg px-2.5 text-[13px] outline-none data-[highlighted]:bg-surface-2",
                  item.isDestructive ? "text-danger" : "text-ink",
                )}
              >
                {item.icon && <span className="text-muted">{item.icon}</span>}
                {item.label}
              </DropdownMenu.Item>
            ))}
          </motion.div>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
