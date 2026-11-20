import { Info, ShieldAlert, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "./cx";

type Tone = "info" | "warning" | "danger";

const TONES: Record<Tone, { box: string; icon: ReactNode; title: string }> = {
  info: {
    box: "border-line bg-surface-2 text-muted",
    icon: <Info size={18} className="text-decorative" />,
    title: "text-ink",
  },
  warning: {
    box: "border-warning/40 bg-warning/8",
    icon: <TriangleAlert size={18} className="text-warning" />,
    title: "text-warning-text",
  },
  danger: {
    box: "border-danger/40 bg-danger/8",
    icon: <ShieldAlert size={18} className="text-danger" />,
    title: "text-danger-text",
  },
};

/** A callout inside a screen: something to know (info) or to fix (warning, danger). */
export function Notice({
  tone = "info",
  title,
  className,
  children,
}: {
  tone?: Tone;
  title?: string;
  className?: string;
  children: ReactNode;
}) {
  const style = TONES[tone];
  return (
    <div className={cx("flex gap-3 rounded-xl border p-3.5", style.box, className)}>
      <span className="mt-px shrink-0">{style.icon}</span>
      <div className="flex min-w-0 flex-col gap-1.5 text-sm leading-relaxed">
        {title && <h4 className={cx("text-base font-semibold", style.title)}>{title}</h4>}
        {children}
      </div>
    </div>
  );
}
