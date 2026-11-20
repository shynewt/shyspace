import type { ReactNode } from "react";
import { cx } from "./cx";

type Tone = "accent" | "neutral" | "warning" | "danger" | "success";

const TONES: Record<Tone, string> = {
  accent: "bg-accent-soft text-accent-text",
  neutral: "bg-surface-2 text-muted",
  warning: "bg-warning/15 text-warning-text",
  danger: "bg-danger/12 text-danger-text",
  success: "bg-success/12 text-success-text",
};

export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return (
    <span
      className={cx(
        "inline-flex items-center gap-1 rounded px-1.5 py-px text-2xs font-semibold tracking-wide uppercase",
        TONES[tone],
      )}
    >
      {children}
    </span>
  );
}
