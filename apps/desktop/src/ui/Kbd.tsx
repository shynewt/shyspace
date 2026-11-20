import type { ReactNode } from "react";

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-line bg-surface-2 px-1.5 font-sans text-[10.5px] text-muted">{children}</kbd>
  );
}
