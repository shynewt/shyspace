import { type HTMLMotionProps, motion } from "motion/react";
import { cx } from "./cx";

type Tone = "accent" | "danger" | "quiet";

const TONES: Record<Tone, string> = {
  accent: "bg-accent text-on-accent",
  danger: "bg-danger text-white",
  /** Something only waiting, like drafts: no colour, never urgent. */
  quiet: "bg-surface-2 text-muted",
};

/**
 * A small pill with a count ("3", "@2", "4 new"). Takes motion props, so it
 * can pop in when the count changes.
 */
export function CountBadge({ tone = "accent", className, ...props }: HTMLMotionProps<"span"> & { tone?: Tone }) {
  return (
    <motion.span
      {...props}
      className={cx(
        "grid h-[18px] min-w-[18px] shrink-0 place-items-center rounded-full px-1.5 text-2xs font-semibold tabular-nums",
        TONES[tone],
        className,
      )}
    />
  );
}
