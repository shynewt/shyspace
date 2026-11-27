import { ArrowLeft } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";
import { api, type CheckAfterRestart, onLinkEnded, type Session } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { Button, Logo, SegmentedControl, tweens } from "../../ui";
import { CreateForm } from "./CreateForm";
import { JoinForm } from "./JoinForm";
import { LinkJoinForm } from "./LinkJoinForm";
import { RestoreForm } from "./RestoreForm";

type Mode = "create" | "join" | "link" | "restore";

const MODES: Array<{ value: Mode; label: string }> = [
  { value: "create", label: "Create" },
  { value: "join", label: "Join" },
  { value: "link", label: "I already use shyspace" },
  { value: "restore", label: "Restore admin" },
];

/** Creating, joining or restoring a workspace; with `onCancel`, from inside the app, to add another. */
export function Onboarding({
  onReady,
  onCancel,
  cancelLabel,
}: {
  onReady: (session: Session) => void;
  onCancel?: () => void;
  cancelLabel?: string;
}) {
  const [mode, setMode] = useState<Mode>("create");
  const [resumedLink, setResumedLink] = useState<CheckAfterRestart | undefined>();
  const [endedLink, setEndedLink] = useState<string | undefined>();
  useEffect(() => {
    api.waitingDeviceLink().then((waiting) => {
      if (!waiting) return;
      setResumedLink(waiting);
      setMode("link");
    }, reportError);
  }, []);
  // A link that ended (at startup too, before any screen listened) says why here.
  useEffect(() => {
    const showEnded = () =>
      api.takeEndedDeviceLink().then((ended) => {
        if (ended === null) return;
        setEndedLink(ended);
        setResumedLink(undefined);
        setMode("link");
      }, reportError);
    void showEnded();
    return onLinkEnded(() => void showEnded());
  }, []);
  return (
    <motion.main
      className="relative grid h-full place-items-center overflow-auto px-6 py-10"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, transition: tweens.base }}
    >
      <Backdrop />
      <div className="relative w-full max-w-[460px]">
        <motion.div
          className="mb-7 flex flex-col items-center text-center"
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
        >
          <Logo size={48} />
          <h1 className="mt-4 text-2xl font-semibold tracking-tight">
            {onCancel ? "Add a workspace" : "Welcome to shyspace"}
          </h1>
          <p className="mt-1.5 text-muted">
            {onCancel
              ? "Create one, join with an invite, or restore one you're the admin of."
              : "Chat, pages and agents, end-to-end encrypted. No server to run."}
          </p>
        </motion.div>

        <motion.div
          className="rounded-2xl border border-line bg-surface p-2 shadow-float"
          initial={{ opacity: 0, y: 18, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          transition={{ delay: 0.05 }}
        >
          <SegmentedControl label="How to start" options={MODES} value={mode} onChange={setMode} width="full" />
          <div className="px-4 pt-5 pb-4">
            <AnimatePresence mode="wait" initial={false}>
              <motion.div
                key={mode}
                initial={{ opacity: 0, x: 12 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: -12 }}
                transition={tweens.base}
              >
                {mode === "create" && <CreateForm onReady={onReady} />}
                {mode === "join" && <JoinForm onReady={onReady} />}
                {mode === "link" && (
                  <LinkJoinForm key={endedLink} onReady={onReady} resumed={resumedLink} ended={endedLink} />
                )}
                {mode === "restore" && <RestoreForm onReady={onReady} />}
              </motion.div>
            </AnimatePresence>
          </div>
        </motion.div>
        {onCancel && (
          <Button variant="ghost" icon={<ArrowLeft size={14} />} onClick={onCancel} className="mx-auto mt-4 flex">
            {cancelLabel ?? "Back"}
          </Button>
        )}
      </div>
    </motion.main>
  );
}

function Backdrop() {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
      <motion.div
        className="absolute -top-40 left-1/2 h-[520px] w-[820px] -translate-x-1/2 rounded-full opacity-60"
        style={{ background: "radial-gradient(closest-side, var(--accent-soft), transparent)" }}
        animate={{ scale: [1, 1.08, 1] }}
        transition={{ duration: 9, repeat: Number.POSITIVE_INFINITY, ease: "easeInOut" }}
      />
    </div>
  );
}
