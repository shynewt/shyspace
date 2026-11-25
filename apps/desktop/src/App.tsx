import { AnimatePresence, motion } from "motion/react";
import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { Onboarding } from "./features/onboarding/Onboarding";
import { OpenFailed } from "./features/shell/OpenFailed";
import { Shell } from "./features/shell/Shell";
import { WorkspacesProvider } from "./features/shell/Workspaces";
import { api, type Session, type WorkspaceLook } from "./lib/api";
import { useLook, useTheme } from "./lib/preferences";
import { errorMessage } from "./lib/toast";
import { useUpdateChecks } from "./lib/updates";
import { useNoBrowserMenu } from "./lib/useContextMenu";
import { useInputModality } from "./lib/useInputModality";
import { Logo, Toasts, tweens } from "./ui";

/** The component gallery, only in development builds (open with #gallery). */
const Gallery = import.meta.env.DEV ? lazy(() => import("./dev/Gallery")) : null;

type Phase =
  | { kind: "loading" }
  | { kind: "onboarding" }
  /** Workspaces are on this device but wouldn't open. */
  | { kind: "failed"; reason: string }
  /** Creating, joining or restoring a workspace from the failed screen, with a way back to it. */
  | { kind: "replacing"; reason: string }
  | { kind: "ready"; session: Session }
  /** Joining, creating or restoring another workspace, with a way back to `session`. */
  | { kind: "adding"; session: Session };

export function App() {
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });
  useNoBrowserMenu();
  useInputModality();
  const [workspaceLook, setWorkspaceLook] = useState<WorkspaceLook | null>(null);
  useTheme();
  useLook(workspaceLook);
  useUpdateChecks();

  const open = useCallback(() => {
    setPhase({ kind: "loading" });
    api.session().then(
      (session) => setPhase(session ? { kind: "ready", session } : { kind: "onboarding" }),
      (error: unknown) => setPhase({ kind: "failed", reason: errorMessage(error) }),
    );
  }, []);
  useEffect(open, [open]);

  const showSession = useCallback(
    (session: Session | null) => setPhase(session ? { kind: "ready", session } : { kind: "onboarding" }),
    [],
  );
  const shown = phase.kind === "ready" ? phase.session : null;
  const addWorkspace = useCallback(() => {
    if (shown) setPhase({ kind: "adding", session: shown });
  }, [shown]);

  if (Gallery && window.location.hash === "#gallery") {
    return (
      <Suspense>
        <Gallery />
      </Suspense>
    );
  }

  return (
    <>
      <AnimatePresence mode="wait">
        {phase.kind === "loading" && (
          <motion.div
            key="loading"
            className="grid h-full place-items-center"
            exit={{ opacity: 0, transition: tweens.exit }}
          >
            <motion.div
              animate={{ scale: [1, 1.06, 1], opacity: [0.7, 1, 0.7] }}
              transition={{ duration: 1.6, repeat: Number.POSITIVE_INFINITY, ease: "easeInOut" }}
            >
              <Logo size={44} />
            </motion.div>
          </motion.div>
        )}
        {phase.kind === "failed" && (
          <OpenFailed
            key="failed"
            reason={phase.reason}
            onRetry={open}
            onSetUpAnother={() => setPhase({ kind: "replacing", reason: phase.reason })}
          />
        )}
        {phase.kind === "replacing" && (
          <Onboarding
            key="replacing"
            onReady={(session) => setPhase({ kind: "ready", session })}
            onCancel={() => setPhase({ kind: "failed", reason: phase.reason })}
          />
        )}
        {phase.kind === "onboarding" && (
          <Onboarding key="onboarding" onReady={(session) => setPhase({ kind: "ready", session })} />
        )}
        {(phase.kind === "ready" || phase.kind === "adding") && (
          <WorkspacesProvider
            key="workspaces"
            session={phase.session}
            onSession={showSession}
            onAdd={addWorkspace}
            onLook={setWorkspaceLook}
          >
            {phase.kind === "adding" ? (
              <Onboarding
                onReady={showSession}
                onCancel={() => setPhase({ kind: "ready", session: phase.session })}
                cancelLabel={`Back to ${phase.session.workspace_name}`}
              />
            ) : (
              <Shell key={phase.session.workspace_id} session={phase.session} />
            )}
          </WorkspacesProvider>
        )}
      </AnimatePresence>
      <Toasts />
    </>
  );
}
