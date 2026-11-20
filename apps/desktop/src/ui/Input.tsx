import type { ComponentProps } from "react";
import { cx } from "./cx";

const FIELD =
  "w-full rounded-lg border border-line bg-bg text-sm text-ink outline-none transition placeholder:text-faint focus:border-accent focus:ring-3 focus:ring-accent/15 disabled:opacity-50";

export function Input({ className, ...props }: ComponentProps<"input">) {
  return <input spellCheck={false} className={cx(FIELD, "h-9 px-3", className)} {...props} />;
}

export function TextArea({ className, ...props }: ComponentProps<"textarea">) {
  return <textarea className={cx(FIELD, "resize-none px-3 py-2 leading-relaxed", className)} {...props} />;
}
