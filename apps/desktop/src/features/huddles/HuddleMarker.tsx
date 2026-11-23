import { Headphones } from "lucide-react";
import { motion } from "motion/react";
import { cx, springs } from "../../ui";
import { useMyHuddle } from "./HuddleContext";
import { namesList } from "./huddleText";
import { useHuddle, useHuddleNames } from "./useHuddle";

/** A sidebar row's huddle, if one is going: how many are in, and how to say it. */
export type HuddleSummary = { count: number; isMine: boolean; label: string };

export function useHuddleSummary(channel: string): HuddleSummary | null {
  const huddle = useHuddle(channel);
  const { mine } = useMyHuddle();
  const nameOf = useHuddleNames();
  if (!huddle || huddle.participants.length === 0) return null;
  const names = namesList(huddle.participants.map((participant) => nameOf(participant.member)));
  return {
    count: huddle.participants.length,
    isMine: mine?.channel === channel,
    label: `huddle with ${names}${huddle.is_full ? ", full" : ""}`,
  };
}

/**
 * A quiet "huddle here" mark: headphones and a count, never a badge. It
 * fades in once when a huddle starts, and the count pops when it changes.
 */
export function HuddleMarker({ summary }: { summary: HuddleSummary }) {
  return (
    <motion.span
      aria-hidden
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className={cx(
        "flex shrink-0 items-center gap-1 text-xs font-normal",
        summary.isMine ? "text-accent-text" : "text-faint",
      )}
    >
      <Headphones size={13} />
      <motion.span
        key={summary.count}
        initial={{ scale: 0.6, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={springs.pop}
      >
        {summary.count}
      </motion.span>
    </motion.span>
  );
}
