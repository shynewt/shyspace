import { ArrowUp, Paperclip, Pencil, Smile, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type RefObject, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { api, type Message } from "../../lib/api";
import { clipboardImageFile, isEmptyPaste } from "../../lib/clipboardImage";
import { characterCount, MAX_MESSAGE_CHARS, SHOW_COUNT_FROM_CHARS } from "../../lib/limits";
import { droppedPageId, isPageDrag } from "../../lib/pageDrag";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Button, cx, EmojiPicker, fade, IconButton, Kbd, Popover, tweens } from "../../ui";
import { AttachmentList, DropHint, UploadProgress } from "./Attachments";
import { useAttachments } from "./useAttachments";
import { useDraft } from "./useDraft";
import { useEditDraft } from "./useEditDraft";
import { type InsertMention, useSuggestions } from "./useSuggestions";

const MAX_HEIGHT_PX = 200;

export function Composer({
  channelId,
  label,
  threadRoot,
  onSent,
  editing = null,
  onEditDone,
  onEditLast,
  onResumeEdit,
  mentionTarget,
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
  /** Opening the conversation again: carry on with an edit left unsaved there. */
  onResumeEdit?: (message: Message) => void;
  /** Filled with this box's way to insert a mention, for things shown around it. */
  mentionTarget?: RefObject<InsertMention | null>;
}) {
  const [text, setText] = useState("");
  const [isFocused, setFocused] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  useFocusOnMount(input);
  const picker = useSuggestions(input, (next) => updateText(next));
  const [isEmojiOpen, setEmojiOpen] = useState(false);
  const { insertMention } = picker;
  useEffect(() => {
    if (!mentionTarget) return;
    mentionTarget.current = insertMention;
    return () => {
      if (mentionTarget.current === insertMention) mentionTarget.current = null;
    };
  }, [mentionTarget, insertMention]);
  const attachments = useAttachments(channelId, input);
  const hasFiles = !editing && attachments.staged.length > 0;
  const sentLength = characterCount(picker.toSend(text).trim());
  const isTooLong = sentLength > MAX_MESSAGE_CHARS;
  const canSend =
    (sentLength > 0 || hasFiles) && !isTooLong && !(hasFiles && attachments.problem) && !attachments.upload;

  const updateText = useCallback((next: string) => setText(next), []);
  // While editing, the draft is set aside and comes back afterwards; a draft
  // stored meanwhile (a reply kept from a notification) joins it there.
  const [setAside, setSetAside] = useState("");
  const conversation = threadRoot ?? channelId;
  const isDraftBack = useDraft(conversation, editing ? setAside : text, editing ? setSetAside : updateText);
  // An edit left unsaved here comes back with its text, not the message's.
  const resumed = useRef<{ messageId: string; text: string } | null>(null);
  const resumeEdit = useCallback(
    (message: Message, savedText: string) => {
      if (!onResumeEdit) return;
      resumed.current = { messageId: message.id, text: savedText };
      onResumeEdit(message);
    },
    [onResumeEdit],
  );
  const editDraft = useEditDraft({
    conversation,
    text,
    isReady: isDraftBack,
    onResume: resumeEdit,
  });
  // Editing starts once per message: later refreshes of it mustn't reset the text.
  const latest = useRef({ editing, text });
  useEffect(() => {
    latest.current = { editing, text };
  }, [editing, text]);
  const editingId = editing?.id;
  // Set when text is put in for editing: the cursor then goes to its end.
  const shouldPutCursorAtEnd = useRef(false);
  // Grows with its content, up to a limit: measured after every render, once new text is in the field.
  useLayoutEffect(() => {
    const element = input.current;
    if (!element) return;
    element.style.height = "0px";
    element.style.height = `${Math.min(element.scrollHeight, MAX_HEIGHT_PX)}px`;
    if (!shouldPutCursorAtEnd.current) return;
    shouldPutCursorAtEnd.current = false;
    element.setSelectionRange(element.value.length, element.value.length);
  });
  const { adopt } = picker;
  const { begin: beginEditDraft } = editDraft;
  // Only the draft from before any editing is set aside: going from one
  // edit straight to another keeps it, rather than the half-done edit.
  const wasEditing = useRef(false);
  useEffect(() => {
    const message = latest.current.editing;
    if (!editingId || !message) {
      wasEditing.current = false;
      return;
    }
    if (!wasEditing.current) setSetAside(latest.current.text);
    wasEditing.current = true;
    const resumedText = resumed.current?.messageId === editingId ? resumed.current.text : null;
    resumed.current = null;
    const next = resumedText ?? adopt(message.text);
    beginEditDraft(editingId, next, resumedText !== null);
    const element = input.current;
    // The same text renders nothing new, so the cursor moves here rather than after rendering.
    if (element?.value === next) element.setSelectionRange(next.length, next.length);
    else shouldPutCursorAtEnd.current = true;
    updateText(next);
    element?.focus();
  }, [editingId, adopt, updateText, beginEditDraft]);

  const finishEditing = () => {
    editDraft.forget();
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
    try {
      if (hasFiles) await attachments.send(picker.toSend(draft), threadRoot);
      else await api.sendMessage(channelId, picker.toSend(draft), threadRoot);
      picker.forgetPicked();
      onSent?.();
    } catch (error) {
      setText(draft);
      reportError(error);
    }
  };

  /** Puts the emoji where the cursor is, replacing any selection, and leaves the cursor after it. */
  const insertEmoji = (emoji: string) => {
    const element = input.current;
    if (!element) return;
    const before = element.value.slice(0, element.selectionStart) + emoji;
    flushSync(() => {
      updateText(before + element.value.slice(element.selectionEnd));
      setEmojiOpen(false);
    });
    // At once, not on the next frame, so a key typed straight after lands after the emoji.
    element.focus();
    element.setSelectionRange(before.length, before.length);
  };

  const isHintShown = editing !== null || (isFocused && text === "");

  return (
    <div className="@container/composer relative shrink-0 px-5 pt-1 pb-7">
      <div className="relative">
        <DropHint isShown={attachments.isDragTarget && !editing} label={label} />
        <AnimatePresence>{picker.list}</AnimatePresence>
        <div className="overflow-hidden rounded-xl border border-line bg-surface shadow-card transition-colors focus-within:border-accent/50 focus-within:ring-3 focus-within:ring-accent/10">
          <AnimatePresence initial={false}>
            {editing && (
              <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={tweens.fast}>
                <div className="flex items-center gap-2 border-b border-line bg-accent-soft/50 px-3 py-1 text-xs text-accent-text">
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
          {!editing && (
            <AttachmentList files={attachments.staged} problem={attachments.problem} onRemove={attachments.remove} />
          )}
          {attachments.upload && <UploadProgress upload={attachments.upload} />}
          <div className="flex items-end gap-2 p-2 pl-3.5">
            <textarea
              ref={input}
              value={text}
              rows={1}
              aria-label={label}
              {...picker.inputProps}
              onFocus={() => {
                setFocused(true);
                attachments.markActive();
              }}
              onPaste={(event) => {
                if (editing) return;
                if (attachments.paste(Array.from(event.clipboardData.files))) event.preventDefault();
                else if (isEmptyPaste(event.clipboardData))
                  clipboardImageFile().then((file) => attachments.paste([file]), reportError);
              }}
              onBlur={() => setFocused(false)}
              onDragOver={(event) => {
                if (!isPageDrag(event)) return;
                event.preventDefault();
                event.dataTransfer.dropEffect = "copy";
              }}
              onDrop={(event) => {
                const pageId = droppedPageId(event);
                if (!pageId) return;
                event.preventDefault();
                picker.insertPage(pageId);
              }}
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
                } else if (event.key === "ArrowUp" && !hasModifier(event) && !text && !editing && onEditLast) {
                  event.preventDefault();
                  onEditLast();
                }
              }}
              placeholder={label}
              className="max-h-[200px] min-h-8 flex-1 resize-none bg-transparent py-[5.5px] text-base leading-[1.5] outline-none placeholder:text-faint"
            />
            {!editing && (
              <IconButton
                label="Attach files"
                icon={<Paperclip size={16} />}
                onClick={attachments.pick}
                className="mb-0.5"
              />
            )}
            <Popover
              open={isEmojiOpen}
              onOpenChange={setEmojiOpen}
              returnFocusTo={input}
              trigger={<IconButton label="Insert emoji" icon={<Smile size={16} />} className="mb-0.5" />}
            >
              <EmojiPicker onSelect={insertEmoji} />
            </Popover>
            <Button
              variant="primary"
              aria-label={editing ? "Save" : "Send"}
              disabled={!canSend}
              onClick={send}
              icon={<ArrowUp size={16} strokeWidth={2.4} />}
              className="w-8 px-0"
            />
          </div>
        </div>
      </div>
      {/* In the space under the box, so showing it never moves the box. */}
      <div className="absolute inset-x-6 bottom-1.5 flex h-[18px] items-center gap-3 overflow-hidden text-2xs text-faint">
        <LengthCount length={sentLength} isTooLong={isTooLong} />
        <AnimatePresence initial={false}>
          {isHintShown && (
            <motion.p
              key={editing ? "editing" : "writing"}
              {...fade}
              className="flex items-center gap-3 @max-composer-hints/composer:hidden"
            >
              {editing ? (
                <>
                  <KeyHint keys="Enter">save</KeyHint>
                  <KeyHint keys="Esc">cancel</KeyHint>
                </>
              ) : (
                <>
                  <KeyHint keys="Enter">send</KeyHint>
                  <KeyHint keys="Shift Enter">new line</KeyHint>
                  <KeyHint keys="↑">edit last</KeyHint>
                </>
              )}
            </motion.p>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

function KeyHint({ keys, children }: { keys: string; children: string }) {
  return (
    <span className="flex items-center gap-1 whitespace-nowrap">
      <Kbd>{keys}</Kbd> {children}
    </span>
  );
}

function hasModifier(event: React.KeyboardEvent): boolean {
  return event.altKey || event.ctrlKey || event.metaKey || event.shiftKey;
}

function LengthCount({ length, isTooLong }: { length: number; isTooLong: boolean }) {
  return (
    <AnimatePresence initial={false}>
      {length >= SHOW_COUNT_FROM_CHARS && (
        <motion.span
          initial={{ opacity: 0, y: 2 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, transition: tweens.exit }}
          className={cx(
            "order-last ml-auto tabular-nums",
            isTooLong ? "font-medium text-danger-text" : "text-warning-text",
          )}
        >
          {isTooLong && "Too long to send: "}
          {length.toLocaleString()} / {MAX_MESSAGE_CHARS.toLocaleString()}
        </motion.span>
      )}
    </AnimatePresence>
  );
}
