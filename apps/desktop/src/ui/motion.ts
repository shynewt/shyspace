import type { Transition } from "motion/react";
import { wasLastInputAKey } from "../lib/useInputModality";

/*
 * Motion tokens: every animation in the app uses these (see Motion in
 * docs/design-system.md). The `@theme` in styles.css has the same numbers for
 * CSS transitions.
 */

/** Fast start, soft landing: for everything that appears or moves. Never ease in. */
export const easeOut: [number, number, number, number] = [0.23, 1, 0.32, 1];

/** In seconds. Exits are faster than entrances. */
export const durations = { instant: 0, exit: 0.08, fast: 0.1, base: 0.15, panel: 0.2 } as const;

export const tweens = {
  instant: { duration: durations.instant },
  /** Floating surfaces and content leaving, panels sliding away. */
  exit: { type: "tween", duration: durations.exit, ease: easeOut },
  /** Hover feedback, a scrim leaving, a docked panel appearing. */
  fast: { type: "tween", duration: durations.fast, ease: easeOut },
  /** Fades and pop-ins: menus, dialogs, content appearing. The default for anything unspecified. */
  base: { type: "tween", duration: durations.base, ease: easeOut },
} satisfies Record<string, Transition>;

/** For things that move in space and may be interrupted. Bounce-free, except `pop`. */
export const springs = {
  /** Panels sliding in: the drawer, the side panel, the docked sidebar. */
  move: { type: "spring", visualDuration: durations.panel, bounce: 0 },
  /** A selection highlight moved by a click, a chevron turning. */
  snappy: { type: "spring", visualDuration: 0.16, bounce: 0 },
  /** Small playful pops: a reaction added, a count changing. */
  pop: { type: "spring", visualDuration: 0.2, bounce: 0.25 },
} satisfies Record<string, Transition>;

/** `transition`, or at once when a key caused the change: keyboard use is never kept waiting. */
export function instantAfterAKey(transition: Transition): Transition {
  return wasLastInputAKey() ? tweens.instant : transition;
}

/** Fade and lift in, fade out: for content that appears in place. */
export const rise = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0, transition: tweens.base },
  exit: { opacity: 0, transition: tweens.exit },
} as const;

/** Grow from slightly smaller: for floating surfaces like dialogs and menus. */
export const pop = {
  initial: { opacity: 0, scale: 0.97 },
  animate: { opacity: 1, scale: 1, transition: tweens.base },
  exit: { opacity: 0, scale: 0.97, transition: tweens.exit },
} as const;

/** For scrims, and content that changes in place. */
export const fade = {
  initial: { opacity: 0 },
  animate: { opacity: 1, transition: tweens.base },
  exit: { opacity: 0, transition: tweens.fast },
} as const;
