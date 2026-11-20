import { ArrowUp, Pencil, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Message } from "../../lib/api";
import { characterCount, MAX_MESSAGE_CHARS, SHOW_COUNT_FROM_CHARS } from "../../lib/limits";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Button, cx, IconButton, Kbd, springs } from "../../ui";
import { useDraft } from "./useDraft";
import { useMentionPicker } from "./useMentionPicker";

const MAX_HEIGHT_PX = 200;

export function Composer({
  channelId,
  label,
  threadRoot,
  onSent,
  editing = null,
  onEditDone,
  onEditLast,
}: {
  channelId: string;
  /** Names the field for screen readers and is its placeholder. */
  label: string;
  /** Reply in this thread instead of posting to the channel. */
  threadRoot: string | null;
  onSent?: () => void;
  /** A message of yours to edit here instead of writing a new one. */
  editing?: Message | null;
  onEditDone?: () => void;
  /** Up in an empty composer: edit your last message. */
  onEditLast?: () => void;
}) {
  const [text, setText] = useState("");
  const input = useRef<HTMLTextAreaElement>(null);
  useFocusOnMount(input);
  const picker = useMentionPicker(input, (next) => updateText(next));
  const sentLength = characterCount(picker.toSend(text).trim());
  const isTooLong = sentLength > MAX_MESSAGE_CHARS;
  const canSend = sentLength > 0 && !isTooLong;

  const updateText = useCallback((next: string) => {
    setText(next);
    // Grows with its content, up to a limit.
    const element = input.current;
    if (!element) return;
    element.style.height = "0px";
    element.style.height = `${Math.min(element.scrollHeight, MAX_HEIGHT_PX)}px`;
  }, []);
  // While editing, the draft is set aside and comes back afterwards.
  const [setAside, setSetAside] = useState("");
  useDraft(threadRoot ?? channelId, editing ? setAside : text, updateText);
  // Editing starts once per message: later refreshes of it mustn't reset the text.
  const latest = useRef({ editing, text });
  useEffect(() => {
    latest.current = { editing, text };
  }, [editing, text]);
  const editingId = editing?.id;
  const { adopt } = picker;
  useEffect(() => {
    const message = latest.current.editing;
    if (!editingId || !message) return;
    setSetAside(latest.current.text);
    updateText(adopt(message.text));
    input.current?.focus();
  }, [editingId, adopt, updateText]);

  const finishEditing = () => {
    updateText(setAside);
    onEditDone?.();
  };

  const saveEdit = async (message: Message) => {
    try {
      if (text.trim() !== picker.adopt(message.text).trim()) await api.editMessage(message.id, picker.toSend(text));
      finishEditing();
    } catch (error) {
      reportError(error);
    }
  };

  const send = async () => {
    if (!canSend) return;
    if (editing) return saveEdit(editing);
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
          <AnimatePresence initial={false}>
            {editing && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.16 }}
              >
                <div className="flex items-center gap-2 border-b border-line bg-accent-soft/50 px-3 py-1 text-[12px] text-accent-text">
                  <Pencil size={13} />
                  <span className="font-medium">Editing message</span>
                  <IconButton
                    label="Cancel editing"
                    size="sm"
                    icon={<X size={13} />}
                    onClick={finishEditing}
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
                } else if (event.key === "Escape" && editing) {
                  event.preventDefault();
                  finishEditing();
                } else if (event.key === "ArrowUp" && !text && !editing && onEditLast) {
                  event.preventDefault();
                  onEditLast();
                }
              }}
              placeholder={label}
              className="max-h-[200px] min-h-[24px] flex-1 resize-none bg-transparent py-1 text-[14px] leading-6 outline-none placeholder:text-faint"
            />
            <Button
              variant="primary"
              aria-label={editing ? "Save" : "Send"}
              disabled={!canSend}
              onClick={send}
              icon={<ArrowUp size={16} strokeWidth={2.4} />}
              className="w-8 px-0"
            />
          </div>
        </motion.div>
      </div>
      <p className="mt-1.5 flex items-center gap-1.5 px-1 text-[11px] text-faint">
        <LengthCount length={sentLength} isTooLong={isTooLong} />
        {editing ? (
          <>
            <Kbd>Enter</Kbd> to save · <Kbd>Esc</Kbd> to cancel
          </>
        ) : (
          <>
            <Kbd>Enter</Kbd> to send · <Kbd>Shift Enter</Kbd> for a new line · <Kbd>↑</Kbd> to edit your last message
          </>
        )}
      </p>
    </div>
  );
}

function LengthCount({ length, isTooLong }: { length: number; isTooLong: boolean }) {
  return (
    <AnimatePresence initial={false}>
      {length >= SHOW_COUNT_FROM_CHARS && (
        <motion.span
          initial={{ opacity: 0, y: 2 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0 }}
          transition={springs.snappy}
          className={cx("order-last ml-auto tabular-nums", isTooLong ? "font-medium text-danger" : "text-warning")}
        >
          {isTooLong && "Too long to send: "}
          {length.toLocaleString()} / {MAX_MESSAGE_CHARS.toLocaleString()}
        </motion.span>
      )}
    </AnimatePresence>
  );
}
