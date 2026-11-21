import { Moon } from "lucide-react";
import { motion } from "motion/react";
import type { Channel } from "../../lib/api";
import { formatTimeIn, isKnownTimeZone, isNight, isWithinWorkingHours, wallClockIn } from "../../lib/localTime";
import { displayName } from "../../lib/people";
import { useMinuteClock } from "../../lib/useMinuteClock";
import { useWorkspace } from "../../lib/workspace";
import { durations } from "../../ui";

/**
 * A quiet line above a 1:1 DM's message box while it's late for the other
 * person: "It's 11:10 PM for Theo · outside their working hours". With
 * working hours shared, it shows outside them; without, from 22:00 to 07:00
 * their time. Information only: it never blocks sending and can't be closed,
 * and it goes when their day starts.
 */
export function LocalTimeHint({ channel }: { channel: Channel | undefined }) {
  const { members } = useWorkspace();
  const now = useMinuteClock();
  const [personId] = channel?.kind === "dm" && channel.dm_members.length === 1 ? channel.dm_members : [];
  const person = members?.find((member) => member.id === personId);
  const zone = person?.details.time_zone ?? "";
  const isShown = (() => {
    if (!person || person.is_bot || !person.active || !isKnownTimeZone(zone)) return false;
    const clock = wallClockIn(zone, now);
    const hours = person.details.working_hours;
    return hours ? !isWithinWorkingHours(hours, clock) : isNight(clock);
  })();
  if (!isShown || !person) return null;
  // Fades in; leaves at once, since nothing is waiting on it.
  return (
    <motion.p
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: durations.panel }}
      className="flex items-center gap-1.5 px-5 pb-1 text-xs text-muted"
    >
      <Moon size={12} className="shrink-0 text-decorative" />
      <span className="truncate">
        It's {formatTimeIn(zone, now)} for {displayName(person.name, person.id)}
        {person.details.working_hours && " · outside their working hours"}
      </span>
    </motion.p>
  );
}
