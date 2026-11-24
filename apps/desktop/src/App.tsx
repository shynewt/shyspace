import { AnimatePresence, motion } from "motion/react";
import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { Onboarding } from "./features/onboarding/Onboarding";
import { Shell } from "./features/shell/Shell";
import { WorkspacesProvider } from "./features/shell/Workspaces";
import { api, type Session, type WorkspaceLook } from "./lib/api";
import { useLook, useTheme } from "./lib/preferences";
import { reportError } from "./lib/toast";
import { useUpdateChecks } from "./lib/updates";
import { useNoBrowserMenu } from "./lib/useContextMenu";
import { useInputModality } from "./lib/useInputModality";
import { Logo, Toasts, tweens } from "./ui";

/** The component gallery, only in development builds (open with #gallery). */
const Gallery = import.meta.env.DEV ? lazy(() => import("./dev/Gallery")) : null;

type Phase =
  | { kind: "loading" }
  | { kind: "onboarding" }
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

  useEffect(() => {
    api.session().then(
      (session) => setPhase(session ? { kind: "ready", session } : { kind: "onboarding" }),
      (error) => {
        reportError(error);
        setPhase({ kind: "onboarding" });
      },
    );
  }, []);

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
