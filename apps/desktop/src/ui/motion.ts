import type { Transition } from "motion/react";

/** Motion presets: every animation in the app uses one of these. */
export const springs = {
  /** UI feedback: buttons, chips, selection pills. */
  snappy: { type: "spring", stiffness: 520, damping: 38, mass: 0.8 },
  /** Panels and dialogs entering. */
  smooth: { type: "spring", stiffness: 380, damping: 36 },
  /** Small playful pops: reactions, counters. */
  pop: { type: "spring", stiffness: 600, damping: 26 },
} satisfies Record<string, Transition>;

export const durations = { fast: 0.12, base: 0.18, slow: 0.3 } as const;

/** Fade and lift in, fade out: for content that appears in place. */
export const rise = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, transition: { duration: durations.fast } },
} as const;

/** Grow from slightly smaller: for floating surfaces like dialogs and menus. */
export const pop = {
  initial: { opacity: 0, scale: 0.96 },
  animate: { opacity: 1, scale: 1 },
  exit: { opacity: 0, scale: 0.97, transition: { duration: durations.fast } },
} as const;

export const fade = {
  initial: { opacity: 0 },
  animate: { opacity: 1 },
  exit: { opacity: 0, transition: { duration: durations.fast } },
} as const;
