import { AlertCircle, Check, Undo2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { dismissToast, reportError, useToasts } from "../lib/toast";

export function Toasts() {
  const toasts = useToasts();
  return (
    <div className="pointer-events-none fixed bottom-5 left-1/2 z-50 flex -translate-x-1/2 flex-col items-center gap-2">
      <AnimatePresence>
        {toasts.map((toast) => (
          <motion.div
            key={toast.id}
            layout
            role="status"
            initial={{ opacity: 0, y: 16, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.95, transition: { duration: 0.15 } }}
            className="pointer-events-auto flex max-w-[520px] items-center rounded-xl border border-line bg-surface text-[13px] shadow-float"
          >
            <button
              type="button"
              onClick={() => dismissToast(toast.id)}
              className="flex items-center gap-2 px-3.5 py-2.5 text-left"
            >
              {toast.tone === "error" ? (
                <AlertCircle size={16} className="shrink-0 text-danger" />
              ) : (
                <Check size={16} className="shrink-0 text-success" />
              )}
              <span className="selectable">{toast.text}</span>
            </button>
            {toast.undo && <UndoButton toastId={toast.id} undo={toast.undo} />}
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}

function UndoButton({ toastId, undo }: { toastId: number; undo: () => Promise<void> }) {
  return (
    <button
      type="button"
      onClick={() => {
        dismissToast(toastId);
        undo().catch(reportError);
      }}
      className="mr-1.5 flex h-7 items-center gap-1 rounded-lg px-2 font-medium text-accent-text transition-colors hover:bg-accent-soft"
    >
      <Undo2 size={13} />
      Undo
    </button>
  );
}
