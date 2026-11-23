import { CircleAlert, CircleArrowUp } from "lucide-react";
import { motion } from "motion/react";
import { errorMessage } from "../../lib/toast";
import { restartToUpdate, useRestartState } from "../../lib/updates";
import { Button, FormError, rise, Tooltip, tweens } from "../../ui";
import { useMyHuddle } from "../huddles/HuddleContext";

/** The app's one restart, and whether a huddle holds it off. */
function useRestart() {
  const { mine } = useMyHuddle();
  const restart = useRestartState();
  const isInHuddle = Boolean(mine);
  // The core also refuses for a huddle this window doesn't show (another workspace's); Restart stays to try again.
  const waitsForHuddle = isInHuddle || restart.kind === "after_huddle";
  return { restart, isInHuddle, waitsForHuddle, run: restartToUpdate };
}

/** "Update ready", docked at the foot of the sidebar until you restart. No dismiss: it stays until acted on. */
export function UpdateCard({ version }: { version: string }) {
  const { restart, isInHuddle, waitsForHuddle, run } = useRestart();
  return (
    <motion.section
      aria-labelledby="update-ready-title"
      {...rise}
      className="mx-2 mb-2 flex flex-col gap-2 rounded-xl border border-line bg-surface p-2.5 shadow-card"
    >
      <div className="flex min-w-0 items-start gap-2">
        <CircleArrowUp size={15} className="mt-0.5 shrink-0 text-accent" />
        <div className="min-w-0">
          <p id="update-ready-title" className="text-sm font-medium">
            Update ready
          </p>
          <p className="text-xs text-muted">
            {restart.kind === "already" ? "Already updating…" : `shyspace ${version} is downloaded.`}
          </p>
        </div>
      </div>
      {restart.kind !== "already" && (
        <>
          {waitsForHuddle && restart.kind !== "restarting" && (
            <p className="text-xs text-muted">Restart after your huddle.</p>
          )}
          <FormError message={restart.kind === "failed" && !isInHuddle ? errorMessage(restart.error) : null} />
          <Button
            variant="primary"
            size="sm"
            disabled={restart.kind === "restarting" || isInHuddle}
            onClick={run}
            className="justify-center"
          >
            {restart.kind === "restarting" ? "Restarting…" : "Restart"}
          </Button>
        </>
      )}
    </motion.section>
  );
}

/** The same, in the header while the sidebar (and so the card) is hidden. A failure shows in its label. */
export function UpdatePill({ version }: { version: string }) {
  const { restart, isInHuddle, waitsForHuddle, run } = useRestart();
  if (restart.kind === "already") return null;
  const failure = restart.kind === "failed" ? errorMessage(restart.error) : null;
  return (
    <motion.div
      role="group"
      aria-label="Update ready"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, transition: tweens.exit }}
      className="flex h-7 shrink-0 items-center gap-1.5 rounded-full border border-line bg-surface pr-0.5 pl-2 text-sm"
    >
      {failure ? (
        <Tooltip label={failure}>
          <span className="flex items-center gap-1.5 text-danger-text">
            <CircleAlert size={13} className="shrink-0" />
            <span className="@max-header-labels/header:hidden">Couldn't update</span>
            <span className="sr-only">: {failure}</span>
          </span>
        </Tooltip>
      ) : (
        <>
          <CircleArrowUp size={13} className="shrink-0 text-accent" />
          <span className="@max-header-labels/header:hidden">
            {waitsForHuddle ? "Update after your huddle" : "Update ready"}
          </span>
        </>
      )}
      <button
        type="button"
        aria-label={`Restart to update to shyspace ${version}`}
        disabled={restart.kind === "restarting" || isInHuddle}
        onClick={run}
        className="flex h-6 items-center rounded-full bg-accent-soft px-2 text-accent-text transition-colors hover:bg-accent-soft/70 disabled:opacity-60"
      >
        {restart.kind === "restarting" ? "Restarting…" : "Restart"}
      </button>
    </motion.div>
  );
}
