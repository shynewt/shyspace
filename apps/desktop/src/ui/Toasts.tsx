import { AnimatePresence, motion } from "motion/react";
import { AlertCircle, Check } from "lucide-react";
import { dismissToast, useToasts } from "../lib/toast";

export function Toasts() {
  const toasts = useToasts();
  return (
    <div className="pointer-events-none fixed bottom-5 left-1/2 z-50 flex -translate-x-1/2 flex-col items-center gap-2">
      <AnimatePresence>
        {toasts.map((toast) => (
          <motion.button
            key={toast.id}
            layout
            initial={{ opacity: 0, y: 16, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.95, transition: { duration: 0.15 } }}
            onClick={() => dismissToast(toast.id)}
            className="pointer-events-auto flex max-w-[520px] items-center gap-2 rounded-xl border border-line bg-surface px-3.5 py-2.5 text-left text-[13px] shadow-float"
          >
            {toast.tone === "error" ? (
              <AlertCircle size={16} className="shrink-0 text-danger" />
            ) : (
              <Check size={16} className="shrink-0 text-success" />
            )}
            <span className="selectable">{toast.text}</span>
          </motion.button>
        ))}
      </AnimatePresence>
    </div>
  );
}
