import { AlarmClock, AlertCircle, ArrowUpRight, Check, Info, Undo2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import { dismissToast, reportError, type Toast, useToasts } from "../lib/toast";

const ICONS: Record<Toast["tone"], ReactNode> = {
  success: <Check size={16} className="shrink-0 text-success" />,
  info: <Info size={16} className="shrink-0 text-muted" />,
  error: <AlertCircle size={16} className="shrink-0 text-danger" />,
  reminder: <AlarmClock size={16} className="shrink-0 text-accent" />,
};

const ACTIONS: Record<NonNullable<Toast["action"]>["kind"], { label: string; icon: ReactNode }> = {
  undo: { label: "Undo", icon: <Undo2 size={13} /> },
  open: { label: "Open", icon: <ArrowUpRight size={13} /> },
};

/** Sits above the message box, so a toast never covers what you're typing. */
export function Toasts() {
  const toasts = useToasts();
  return (
    <div className="pointer-events-none fixed bottom-24 left-1/2 z-50 flex -translate-x-1/2 flex-col items-center gap-2">
      <AnimatePresence>
        {toasts.map((toast) => (
          <motion.div
            key={toast.id}
            layout
            role={toast.tone === "error" ? "alert" : "status"}
            initial={{ opacity: 0, y: 16, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.95, transition: { duration: 0.15 } }}
            className="pointer-events-auto flex max-w-[520px] items-center rounded-xl border border-line bg-surface text-sm shadow-float"
          >
            <button
              type="button"
              onClick={() => dismissToast(toast.id)}
              className="flex items-center gap-2 px-3.5 py-2.5 text-left"
            >
              {ICONS[toast.tone]}
              <span className="selectable">{toast.text}</span>
            </button>
            {toast.action && <ActionButton toastId={toast.id} action={toast.action} />}
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}

function ActionButton({ toastId, action }: { toastId: number; action: NonNullable<Toast["action"]> }) {
  const { label, icon } = ACTIONS[action.kind];
  return (
    <button
      type="button"
      onClick={() => {
        dismissToast(toastId);
        Promise.resolve(action.run()).catch(reportError);
      }}
      className="mr-1.5 flex h-7 items-center gap-1 rounded-lg px-2 font-medium text-accent-text transition-colors hover:bg-accent-soft"
    >
      {icon}
      {label}
    </button>
  );
}
