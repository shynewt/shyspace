import { AlarmClock, BookmarkCheck } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type { SavedMark } from "../../lib/api";
import { describeReminder } from "../../lib/reminders";
import { useWorkspace } from "../../lib/workspace";
import { cx, springs, Tooltip } from "../../ui";

/** A small bookmark or alarm by a message you saved; opens Later. Hidden once done. */
export function SavedMarker({ saved }: { saved: SavedMark | null }) {
  const { navigate } = useWorkspace();
  const isShown = saved !== null && !saved.is_done;
  const remindMs = saved?.remind_ms ?? null;
  const isDue = remindMs !== null && remindMs <= Date.now();
  const description =
    remindMs === null
      ? "Saved for later"
      : isDue
        ? "Reminder due"
        : `Reminder ${describeReminder(remindMs, new Date())}`;
  return (
    <AnimatePresence initial={false}>
      {isShown && (
        <motion.span
          initial={{ scale: 0.4, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          exit={{ scale: 0.4, opacity: 0 }}
          transition={springs.pop}
          className="inline-grid"
        >
          <Tooltip label={description}>
            <button
              type="button"
              aria-label={`${description}. Open Later`}
              onClick={() => navigate({ kind: "later" })}
              className={cx(
                "grid size-5 place-items-center rounded-md transition-colors hover:bg-surface-2",
                isDue ? "text-danger" : "text-accent-text",
              )}
            >
              {remindMs === null ? <BookmarkCheck size={13} /> : <AlarmClock size={13} />}
            </button>
          </Tooltip>
        </motion.span>
      )}
    </AnimatePresence>
  );
}
