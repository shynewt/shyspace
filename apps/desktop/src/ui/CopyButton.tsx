import { Check, Copy } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { reportError } from "../lib/toast";
import { IconButton } from "./IconButton";
import { springs } from "./motion";

const COPIED_FEEDBACK_MS = 1500;

/**
 * Copies `value`. Its icon turns into a check for a moment, and a polite live
 * region beside it (not inside: a button's content isn't reliably announced)
 * says "Copied". Clicking again restarts the moment; it's never disabled.
 */
export function CopyButton({ value, label, className }: { value: string; label: string; className?: string }) {
  const [isCopied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = () =>
    navigator.clipboard.writeText(value).then(() => {
      clearTimeout(timer.current);
      setCopied(true);
      timer.current = setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS);
    }, reportError);

  return (
    <>
      <IconButton
        label={label}
        onClick={copy}
        className={className}
        icon={
          <AnimatePresence mode="wait" initial={false}>
            <motion.span
              key={isCopied ? "done" : "copy"}
              data-copied={isCopied || undefined}
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
      <span role="status" className="sr-only">
        {isCopied ? "Copied" : ""}
      </span>
    </>
  );
}
