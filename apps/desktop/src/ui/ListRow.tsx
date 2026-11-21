import type { ReactNode } from "react";
import { cx } from "./cx";

/**
 * A clickable row in a list (Inbox, Later): a leading avatar or icon, a small
 * line saying who and where, and a one-line preview. Actions in `trailing`
 * sit outside the button, so they stay separate controls.
 */
export function ListRow({
  leading,
  meta,
  onClick,
  trailing,
  isHighlighted = false,
  className,
  children,
}: {
  leading: ReactNode;
  meta: ReactNode;
  onClick: () => void;
  trailing?: ReactNode;
  /** Shows the hover background, e.g. while the row's menu is open. */
  isHighlighted?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      className={cx(
        "group flex items-center gap-1 rounded-xl transition-colors hover:bg-surface-2",
        isHighlighted && "bg-surface-2",
        trailing !== undefined && "pr-2",
        className,
      )}
    >
      <button
        type="button"
        onClick={onClick}
        className="focus-ring-inset flex min-w-0 flex-1 items-start gap-3 rounded-xl px-3 py-2.5 text-left"
      >
        <span className="shrink-0">{leading}</span>
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-1.5 text-sm text-muted">{meta}</span>
          <span className="mt-0.5 block truncate text-base">{children}</span>
        </span>
      </button>
      {trailing}
    </div>
  );
}
