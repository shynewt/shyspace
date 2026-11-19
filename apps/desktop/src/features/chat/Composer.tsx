import { ArrowUp, CornerUpLeft, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, type Message } from "../../lib/api";
import { plainPreview } from "../../lib/markdown";
import { displayName } from "../../lib/people";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { useWorkspace } from "../../lib/workspace";
import { Button, IconButton, Kbd } from "../../ui";

const MAX_HEIGHT_PX = 200;

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
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const [text, setText] = useState("");
  const input = useRef<HTMLTextAreaElement>(null);
  const canSend = text.trim().length > 0;
  useFocusOnMount(input);

  useEffect(() => {
    if (replyTo) input.current?.focus();
  }, [replyTo]);

  const updateText = (next: string) => {
    setText(next);
    // Grows with its content, up to a limit.
    const element = input.current;
    if (!element) return;
    element.style.height = "0px";
    element.style.height = `${Math.min(element.scrollHeight, MAX_HEIGHT_PX)}px`;
  };

  const send = async () => {
    if (!canSend) return;
    const draft = text;
    setText("");
    if (input.current) input.current.style.height = "";
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
      <motion.div
        layout
        className="overflow-hidden rounded-xl border border-line bg-surface shadow-card transition-colors focus-within:border-accent/50 focus-within:ring-3 focus-within:ring-accent/10"
      >
        <AnimatePresence initial={false}>
          {replyTo && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={{ duration: 0.16 }}
            >
              <div className="flex items-center gap-2 border-b border-line bg-surface-2/60 px-3 py-1 text-[12px] text-muted">
                <CornerUpLeft size={13} />
                <span>
                  Replying to{" "}
                  <b className="font-semibold text-ink">{displayName(replyTo.author_name, replyTo.author)}</b>
                </span>
                <span className="truncate text-faint">{plainPreview(replyTo.text, (id) => titles.get(id))}</span>
                <IconButton
                  label="Cancel reply"
                  size="sm"
                  icon={<X size={13} />}
                  onClick={onCancelReply}
                  className="ml-auto"
                />
              </div>
            </motion.div>
          )}
        </AnimatePresence>
        <div className="flex items-end gap-2 p-2 pl-3.5">
          <textarea
            ref={input}
            value={text}
            rows={1}
            aria-label={`Message #${channelName}`}
            onChange={(event) => updateText(event.target.value)}
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
          <Button
            variant="primary"
            aria-label="Send"
            disabled={!canSend}
            onClick={send}
            icon={<ArrowUp size={16} strokeWidth={2.4} />}
            className="w-8 px-0"
          />
        </div>
      </motion.div>
      <p className="mt-1.5 flex items-center gap-1.5 px-1 text-[11px] text-faint">
        <Kbd>Enter</Kbd> to send · <Kbd>Shift Enter</Kbd> for a new line · Markdown works
      </p>
    </div>
  );
}
