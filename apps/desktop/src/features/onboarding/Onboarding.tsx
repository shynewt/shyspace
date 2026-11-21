import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import type { Session } from "../../lib/api";
import { Logo, SegmentedControl } from "../../ui";
import { CreateForm } from "./CreateForm";
import { JoinForm } from "./JoinForm";
import { RestoreForm } from "./RestoreForm";

type Mode = "create" | "join" | "restore";

const MODES: Array<{ value: Mode; label: string }> = [
  { value: "create", label: "Create" },
  { value: "join", label: "Join" },
  { value: "restore", label: "Restore admin" },
];

export function Onboarding({ onReady }: { onReady: (session: Session) => void }) {
  const [mode, setMode] = useState<Mode>("create");
  return (
    <motion.main
      className="relative grid h-full place-items-center overflow-auto px-6 py-10"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, transition: { duration: 0.2 } }}
    >
      <Backdrop />
      <div className="relative w-full max-w-[460px]">
        <motion.div
          className="mb-7 flex flex-col items-center text-center"
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
        >
          <Logo size={48} />
          <h1 className="mt-4 text-2xl font-semibold tracking-tight">Welcome to shyspace</h1>
          <p className="mt-1.5 text-muted">Chat, pages and agents, end-to-end encrypted. No server to run.</p>
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
                transition={{ duration: 0.18, ease: "easeOut" }}
              >
                {mode === "create" && <CreateForm onReady={onReady} />}
                {mode === "join" && <JoinForm onReady={onReady} />}
                {mode === "restore" && <RestoreForm onReady={onReady} />}
              </motion.div>
            </AnimatePresence>
          </div>
        </motion.div>
      </div>
    </motion.main>
  );
}

function Backdrop() {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
      <motion.div
        className="absolute -top-40 left-1/2 h-[520px] w-[820px] -translate-x-1/2 rounded-full opacity-60 blur-3xl"
        style={{ background: "radial-gradient(closest-side, var(--accent-soft), transparent)" }}
        animate={{ scale: [1, 1.08, 1] }}
        transition={{ duration: 9, repeat: Number.POSITIVE_INFINITY, ease: "easeInOut" }}
      />
    </div>
  );
}
