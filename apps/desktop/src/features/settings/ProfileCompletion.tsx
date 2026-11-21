import { X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type { ProfileDetails } from "../../lib/api";
import { usePreferences } from "../../lib/preferences";
import { completedProfileSteps, PROFILE_STEPS } from "../../lib/profileFields";
import { IconButton, springs } from "../../ui";

/**
 * A quiet line suggesting what to add, with how much is done. It goes once
 * the profile is complete, or for good on this device once dismissed.
 */
export function ProfileCompletion({ details }: { details: ProfileDetails | undefined }) {
  const [{ isProfileNudgeDismissed }, updatePreferences] = usePreferences();
  const done = details ? completedProfileSteps(details) : PROFILE_STEPS;
  const isShown = !isProfileNudgeDismissed && done < PROFILE_STEPS;
  return (
    <AnimatePresence initial={false}>
      {isShown && (
        <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }}>
          <div className="mb-5 flex items-start gap-3 rounded-xl bg-surface-2 py-2.5 pr-2 pl-3.5">
            <div className="flex min-w-0 flex-1 flex-col gap-2 pt-0.5">
              <p className="text-sm text-muted">
                Add your role and time zone so people know who you are and when you're around.
              </p>
              <div
                role="progressbar"
                aria-label="Profile completed"
                aria-valuemin={0}
                aria-valuemax={PROFILE_STEPS}
                aria-valuenow={done}
                aria-valuetext={`${done} of ${PROFILE_STEPS} done`}
                className="h-1 w-40 overflow-hidden rounded-full bg-surface-3"
              >
                <motion.div
                  className="h-full origin-left rounded-full bg-accent"
                  initial={false}
                  animate={{ scaleX: done / PROFILE_STEPS }}
                  transition={springs.move}
                />
              </div>
            </div>
            <IconButton
              label="Dismiss"
              icon={<X size={14} />}
              size="sm"
              onClick={() => updatePreferences({ isProfileNudgeDismissed: true })}
            />
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
