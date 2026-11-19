import { motion } from "motion/react";
import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { cx } from "./cx";
import { springs } from "./motion";
import { Tooltip } from "./Tooltip";

type IconButtonProps = Omit<ComponentPropsWithoutRef<typeof motion.button>, "children" | "aria-label"> & {
  /** Read by screen readers and shown as the tooltip. */
  label: string;
  icon: ReactNode;
  size?: "sm" | "md";
  isActive?: boolean;
};

export function IconButton({ label, icon, size = "md", isActive = false, className, ...props }: IconButtonProps) {
  return (
    <Tooltip label={label}>
      <motion.button
        type="button"
        aria-label={label}
        whileTap={{ scale: 0.9 }}
        transition={springs.snappy}
        className={cx(
          "grid shrink-0 place-items-center rounded-md transition-colors disabled:opacity-40",
          size === "sm" ? "h-6 w-6" : "h-7 w-7",
          isActive ? "bg-accent-soft text-accent-text" : "text-muted hover:bg-surface-2 hover:text-ink",
          className,
        )}
        {...props}
      >
        {icon}
      </motion.button>
    </Tooltip>
  );
}
