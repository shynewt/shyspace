import { motion } from "motion/react";
import { ContextMenu as RadixContextMenu } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "./cx";
import { MENU_ITEM, MENU_SURFACE, type MenuAction } from "./Menu";
import { pop } from "./motion";

/**
 * Actions on right-click, shown at the pointer, styled like `Menu`. Marked
 * so a menu for what it sits inside (a message) doesn't open as well.
 */
export function ContextMenu({ items, children }: { items: MenuAction[]; children: ReactNode }) {
  return (
    <RadixContextMenu.Root>
      <RadixContextMenu.Trigger asChild data-own-menu="">
        {children}
      </RadixContextMenu.Trigger>
      <RadixContextMenu.Portal>
        <RadixContextMenu.Content asChild>
          <motion.div {...pop} className={cx(MENU_SURFACE, "origin-(--radix-context-menu-content-transform-origin)")}>
            {items.map((item) => (
              <RadixContextMenu.Item
                key={item.label}
                onSelect={item.onSelect}
                className={cx(MENU_ITEM, item.isDestructive ? "text-danger-text" : "text-ink")}
              >
                {item.icon && <span className={item.isDestructive ? undefined : "text-muted"}>{item.icon}</span>}
                {item.label}
              </RadixContextMenu.Item>
            ))}
          </motion.div>
        </RadixContextMenu.Content>
      </RadixContextMenu.Portal>
    </RadixContextMenu.Root>
  );
}
