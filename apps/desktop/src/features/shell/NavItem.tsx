import { motion } from "motion/react";
import type { ReactNode } from "react";
import { sameView, useWorkspace, type View } from "../../lib/workspace";
import { cx, springs } from "../../ui";

export const INDENT_PX = 14;

export function NavItem({
  view,
  icon,
  label,
  depth = 0,
  leading,
  trailing,
}: {
  view: View;
  icon: ReactNode;
  label: string;
  depth?: number;
  /** Rendered before the item, outside its button (like a fold toggle). */
  leading?: ReactNode;
  trailing?: ReactNode;
}) {
  const { view: current, navigate } = useWorkspace();
  const isActive = sameView(current, view);
  return (
    <div className="group relative flex items-center" style={{ paddingLeft: depth * INDENT_PX }}>
      {isActive && (
        <motion.div
          layoutId="nav-active"
          transition={springs.snappy}
          className="absolute inset-y-0 right-0 rounded-lg bg-surface shadow-card"
          style={{ left: depth * INDENT_PX }}
        />
      )}
      {leading}
      <button
        type="button"
        aria-current={isActive ? "page" : undefined}
        onClick={() => navigate(view)}
        className={cx(
          "relative flex h-8 min-w-0 flex-1 items-center gap-2 rounded-lg px-2.5 text-[13.5px] transition-colors",
          isActive ? "font-medium text-ink" : "text-muted hover:bg-surface-2/70 hover:text-ink",
        )}
      >
        <span className={isActive ? "text-accent" : "text-faint"}>{icon}</span>
        <span className="truncate">{label}</span>
      </button>
      {trailing}
    </div>
  );
}
