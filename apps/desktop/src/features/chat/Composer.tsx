import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ArrowUp, CornerUpLeft, X } from "lucide-react";
import { api, type Message } from "../lib/api";
import { plainPreview } from "../lib/markdown";
import { displayName } from "../lib/people";
import { useWorkspace } from "../lib/workspace";
import { reportError } from "../lib/toast";

const MAX_HEIGHT = 200;

export function Composer({
  channelId,
  channelName,
  replyTo,
  onCancelReply,
  onSent,
}: {
  channelId: string;
  channelName: string;
  replyTo: Message | null;
  onCancelReply: () => void;
  onSent: () => void;
}) {
  const { pages } = useWorkspace();
  const titles = new Map(pages.map((page) => [page.id, page.title || "Untitled"]));
  const [text, setText] = useState("");
  const input = useRef<HTMLTextAreaElement>(null);
  const canSend = text.trim().length > 0;

  useEffect(() => {
    const element = input.current;
    if (!element) return;
    element.style.height = "0px";
    element.style.height = `${Math.min(element.scrollHeight, MAX_HEIGHT)}px`;
  }, [text]);

  useEffect(() => {
    if (replyTo) input.current?.focus();
  }, [replyTo]);

  const send = async () => {
    if (!canSend) return;
    const draft = text;
    setText("");
    try {
      await api.sendMessage(channelId, draft, replyTo?.id ?? null);
      onSent();
    } catch (error) {
      setText(draft);
      reportError(error);
    }
  };

  return (
    <div className="shrink-0 px-5 pt-1 pb-4">
      <motion.div layout className="overflow-hidden rounded-xl border border-line bg-surface shadow-card transition-colors focus-within:border-accent/50 focus-within:ring-3 focus-within:ring-accent/10">
        <AnimatePresence initial={false}>
          {replyTo && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={{ duration: 0.16 }}
            >
              <div className="flex items-center gap-2 border-b border-line bg-surface-2/60 px-3 py-1.5 text-[12px] text-muted">
                <CornerUpLeft size={13} />
                <span>
                  Replying to <b className="font-semibold text-ink">{displayName(replyTo.author_name, replyTo.author)}</b>
                </span>
                <span className="truncate text-faint">{plainPreview(replyTo.text, (id) => titles.get(id))}</span>
                <button onClick={onCancelReply} aria-label="Cancel reply" className="ml-auto rounded p-0.5 hover:bg-surface-3 hover:text-ink">
                  <X size={13} />
                </button>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
        <div className="flex items-end gap-2 p-2 pl-3.5">
          <textarea
            ref={input}
            value={text}
            rows={1}
            autoFocus
            onChange={(event) => setText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                event.preventDefault();
                send();
              }
              if (event.key === "Escape" && replyTo) onCancelReply();
            }}
            placeholder={`Message #${channelName}`}
            className="max-h-[200px] min-h-[24px] flex-1 resize-none bg-transparent py-1 text-[14px] leading-6 outline-none placeholder:text-faint"
          />
          <motion.button
            onClick={send}
            disabled={!canSend}
            aria-label="Send"
            animate={{ scale: canSend ? 1 : 0.85, opacity: canSend ? 1 : 0.4 }}
            whileTap={canSend ? { scale: 0.9 } : undefined}
            className="grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-accent text-white disabled:cursor-default"
          >
            <ArrowUp size={16} strokeWidth={2.4} />
          </motion.button>
        </div>
      </motion.div>
      <p className="mt-1.5 px-1 text-[11px] text-faint">
        <b className="font-medium">Enter</b> to send · <b className="font-medium">Shift Enter</b> for a new line · Markdown works
      </p>
    </div>
  );
}
