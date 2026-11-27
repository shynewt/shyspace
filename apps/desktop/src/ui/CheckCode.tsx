import { cx } from "./cx";

/** How a check code is read aloud: its two groups, with a pause between ("482, 917"). */
export function spokenCheckCode(code: string): string {
  return code.split(" ").join(", ");
}

/** A six-digit check code in large grouped digits ("482 917"). */
export function CheckCode({ code, className }: { code: string; className?: string }) {
  return (
    <span className={cx("font-mono text-3xl font-semibold tracking-wider text-ink tabular-nums", className)}>
      <span aria-hidden="true">{code}</span>
      <span className="sr-only">{spokenCheckCode(code)}</span>
    </span>
  );
}
