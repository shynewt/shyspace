import type { ReactNode } from "react";
import { cx } from "./cx";

/** A still line saying how something stands (never animated), with an optional small icon. */
export function StatusLine({
  icon,
  children,
  className,
}: {
  icon?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <p role="status" className={cx("flex items-center gap-1.5 text-xs text-muted", className)}>
      {icon && <span className="shrink-0 text-decorative">{icon}</span>}
      <span>{children}</span>
    </p>
  );
}
