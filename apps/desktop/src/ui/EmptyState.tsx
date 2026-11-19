import { motion } from "motion/react";
import type { ReactNode } from "react";
import { rise } from "./motion";

export function EmptyState({
  icon,
  title,
  children,
  action,
}: {
  icon: ReactNode;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <motion.div {...rise} className="grid h-full place-items-center px-6 py-10 text-center">
      <div className="flex max-w-[340px] flex-col items-center">
        <div className="grid h-12 w-12 place-items-center rounded-2xl bg-accent-soft text-accent-text">{icon}</div>
        <h2 className="mt-3 text-[15px] font-semibold">{title}</h2>
        {children && <p className="mt-1 text-[13px] leading-relaxed text-muted">{children}</p>}
        {action && <div className="mt-4">{action}</div>}
      </div>
    </motion.div>
  );
}
