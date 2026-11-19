import { motion } from "motion/react";
import { cx } from "./cx";

export function Spinner({ size = 16, className }: { size?: number; className?: string }) {
  return (
    <motion.span
      role="status"
      aria-label="Loading"
      className={cx("inline-block shrink-0 rounded-full border-2 border-current border-t-transparent", className)}
      style={{ width: size, height: size }}
      animate={{ rotate: 360 }}
      transition={{ duration: 0.8, repeat: Number.POSITIVE_INFINITY, ease: "linear" }}
    />
  );
}
