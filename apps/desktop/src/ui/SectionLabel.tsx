import type { ReactNode } from "react";

/** A small uppercase heading for groups in lists and panels. */
export function SectionLabel({ children, action }: { children: ReactNode; action?: ReactNode }) {
  return (
    <div className="group flex h-7 items-center justify-between px-2.5">
      <span className="text-xs font-semibold tracking-wide text-faint uppercase">{children}</span>
      {action}
    </div>
  );
}
