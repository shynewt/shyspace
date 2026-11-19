import { ArrowUp } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useRef, useState } from "react";
import { api } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Button, Kbd } from "../../ui";
import { useDraft } from "./useDraft";
import { useMentionPicker } from "./useMentionPicker";

const MAX_HEIGHT_PX = 200;

export function Composer({
  channelId,
  label,
  threadRoot,
  onSent,
}: {
  channelId: string;
  /** Names the field for screen readers and is its placeholder. */
  label: string;
  /** Reply in this thread instead of posting to the channel. */
  threadRoot: string | null;
  onSent?: () => void;
}) {
  const [text, setText] = useState("");
  const input = useRef<HTMLTextAreaElement>(null);
  const canSend = text.trim().length > 0;
  useFocusOnMount(input);
  const picker = useMentionPicker(input, (next) => updateText(next));

  const updateText = useCallback((next: string) => {
    setText(next);
    // Grows with its content, up to a limit.
    const element = input.current;
    if (!element) return;
    element.style.height = "0px";
    element.style.height = `${Math.min(element.scrollHeight, MAX_HEIGHT_PX)}px`;
  }, []);
  useDraft(threadRoot ?? channelId, text, updateText);

  const send = async () => {
    if (!canSend) return;
    const draft = text;
    setText("");
    if (input.current) input.current.style.height = "";
    try {
      await api.sendMessage(channelId, picker.toSend(draft), threadRoot);
      picker.forgetPicked();
      onSent?.();
    } catch (error) {
      setText(draft);
      reportError(error);
    }
  };

  return (
    <div className="shrink-0 px-5 pt-1 pb-4">
      <div className="relative">
        <AnimatePresence>{picker.list}</AnimatePresence>
        <motion.div
          layout
          className="overflow-hidden rounded-xl border border-line bg-surface shadow-card transition-colors focus-within:border-accent/50 focus-within:ring-3 focus-within:ring-accent/10"
        >
          <div className="flex items-end gap-2 p-2 pl-3.5">
            <textarea
              ref={input}
              value={text}
              rows={1}
              aria-label={label}
              {...picker.inputProps}
              onChange={(event) => {
                updateText(event.target.value);
                picker.onTextChange(event.target.value, event.target.selectionStart);
              }}
              onKeyDown={(event) => {
                if (picker.onKeyDown(event)) return;
                if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                  event.preventDefault();
                  send();
                }
              }}
              placeholder={label}
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
      </div>
      <p className="mt-1.5 flex items-center gap-1.5 px-1 text-[11px] text-faint">
        <Kbd>Enter</Kbd> to send · <Kbd>Shift Enter</Kbd> for a new line · Markdown works
      </p>
    </div>
  );
}
