import { Check, Copy } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";
import { reportError } from "../lib/toast";
import { IconButton } from "./IconButton";
import { springs } from "./motion";

const COPIED_FEEDBACK_MS = 1400;

/** Read-only text with a copy button, for codes and commands. */
export function CopyField({ value, label = "Copy" }: { value: string; label?: string }) {
  const [isCopied, setCopied] = useState(false);

  useEffect(() => {
    if (!isCopied) return;
    const timer = setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS);
    return () => clearTimeout(timer);
  }, [isCopied]);

  const copy = () =>
    navigator.clipboard.writeText(value).then(
      () => setCopied(true),
      (error) => reportError(error),
    );

  return (
    <div className="relative">
      <pre className="selectable max-h-28 overflow-auto rounded-lg border border-line bg-surface p-2 pr-10 font-mono text-[11px] leading-relaxed break-all whitespace-pre-wrap text-muted">
        {value}
      </pre>
      <div className="absolute top-1.5 right-1.5">
        <IconButton
          label={isCopied ? "Copied" : label}
          onClick={copy}
          className="bg-surface-2"
          icon={
            <AnimatePresence mode="wait" initial={false}>
              <motion.span
                key={isCopied ? "done" : "copy"}
                initial={{ scale: 0.5, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                exit={{ scale: 0.5, opacity: 0 }}
                transition={springs.pop}
              >
                {isCopied ? <Check size={13} className="text-success" /> : <Copy size={13} />}
              </motion.span>
            </AnimatePresence>
          }
        />
      </div>
    </div>
  );
}
