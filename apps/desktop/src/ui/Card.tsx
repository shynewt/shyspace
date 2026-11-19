import type { ReactNode } from "react";
import { cx } from "./cx";

/** A quiet bordered box for grouped content in panels. */
export function Card({ highlighted = false, children }: { highlighted?: boolean; children: ReactNode }) {
  return (
    <div
      className={cx("rounded-xl border p-3", highlighted ? "border-accent/40 bg-accent-soft/40" : "border-line bg-bg")}
    >
      {children}
    </div>
  );
}
