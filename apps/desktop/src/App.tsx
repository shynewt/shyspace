import { AnimatePresence, motion } from "motion/react";
import { lazy, Suspense, useEffect, useState } from "react";
import { Onboarding } from "./features/onboarding/Onboarding";
import { Shell } from "./features/shell/Shell";
import { api, type Session } from "./lib/api";
import { reportError } from "./lib/toast";
import { Logo, Toasts } from "./ui";

/** The component gallery, only in development builds (open with #gallery). */
const Gallery = import.meta.env.DEV ? lazy(() => import("./dev/Gallery")) : null;

type Phase = { kind: "loading" } | { kind: "onboarding" } | { kind: "ready"; session: Session };

export function App() {
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });

  useEffect(() => {
    api.session().then(
      (session) => setPhase(session ? { kind: "ready", session } : { kind: "onboarding" }),
      (error) => {
        reportError(error);
        setPhase({ kind: "onboarding" });
      },
    );
  }, []);

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
            exit={{ opacity: 0, scale: 0.96 }}
            transition={{ duration: 0.2 }}
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
        {phase.kind === "ready" && <Shell key="shell" session={phase.session} />}
      </AnimatePresence>
      <Toasts />
    </>
  );
}
