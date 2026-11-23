import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import { springs, tweens } from "./motion";

/**
 * A few overlapping avatars and "+N" for the rest: who's somewhere, at a
 * glance. Someone arriving pops in; someone leaving fades, and the rest close
 * the gap at once (no layout animation).
 */
export function AvatarStack({
  avatars,
  limit = 3,
}: {
  avatars: Array<{ key: string; avatar: ReactNode }>;
  limit?: number;
}) {
  const shown = avatars.slice(0, limit);
  const more = avatars.length - shown.length;
  return (
    <span aria-hidden className="flex items-center gap-1">
      <span className="flex items-center -space-x-1">
        <AnimatePresence initial={false}>
          {shown.map(({ key, avatar }) => (
            <motion.span
              key={key}
              initial={{ scale: 0.6, opacity: 0 }}
              animate={{ scale: 1, opacity: 1, transition: springs.pop }}
              exit={{ opacity: 0, transition: tweens.exit }}
              className="flex rounded-[34%] ring-2 ring-surface"
            >
              {avatar}
            </motion.span>
          ))}
        </AnimatePresence>
      </span>
      {more > 0 && <span className="text-xs text-faint">+{more}</span>}
    </span>
  );
}
