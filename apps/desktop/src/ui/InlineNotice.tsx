import { ShieldAlert, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "./cx";

const TONES = {
  warning: { box: "bg-warning/8 text-warning-text", icon: <TriangleAlert size={13} /> },
  danger: { box: "bg-danger/8 text-danger-text", icon: <ShieldAlert size={13} /> },
};

/**
 * A one-line problem and the one thing that fixes it, for narrow places like
 * the huddle bar, where a `Notice` is too big.
 */
export function InlineNotice({
  tone,
  children,
  action,
}: {
  tone: keyof typeof TONES;
  children: ReactNode;
  action?: { label: string; onClick: () => void };
}) {
  return (
    <div className={cx("flex gap-2 rounded-lg px-2 py-1.5 text-xs leading-snug", TONES[tone].box)}>
      <span className="mt-px shrink-0">{TONES[tone].icon}</span>
      <div className="min-w-0">
        {children}
        {action && (
          <button
            type="button"
            onClick={action.onClick}
            className="mt-1 block font-medium text-ink underline-offset-2 hover:underline"
          >
            {action.label}
          </button>
        )}
      </div>
    </div>
  );
}
