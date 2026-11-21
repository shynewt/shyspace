import { ArrowDown, ArrowUp } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { Button, rise } from "../../ui";

/**
 * Floating buttons over the message list: up to the first unread message,
 * and down to messages that arrived while you read further up. Screen
 * readers hear them politely, not as alerts.
 */
export function UnreadPills({
  isJumpToUnreadShown,
  newCount,
  onJumpToUnread,
  onJumpToNewest,
}: {
  isJumpToUnreadShown: boolean;
  /** Messages from others since you scrolled away from the newest; 0 hides the pill. */
  newCount: number;
  onJumpToUnread: () => void;
  onJumpToNewest: () => void;
}) {
  return (
    <>
      <div aria-live="polite" className="pointer-events-none absolute inset-x-0 top-3 z-10 flex justify-center">
        <AnimatePresence>
          {isJumpToUnreadShown && (
            <motion.div key="unread" {...rise} className="pointer-events-auto">
              <Button size="sm" variant="primary" icon={<ArrowUp size={13} />} onClick={onJumpToUnread}>
                Jump to unread
              </Button>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
      <div aria-live="polite" className="pointer-events-none absolute inset-x-0 bottom-3 z-10 flex justify-center">
        <AnimatePresence>
          {newCount > 0 && (
            <motion.div key="new" {...rise} className="pointer-events-auto">
              <Button size="sm" variant="primary" icon={<ArrowDown size={13} />} onClick={onJumpToNewest}>
                {newCount === 1 ? "1 new message" : `${newCount} new messages`}
              </Button>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </>
  );
}
