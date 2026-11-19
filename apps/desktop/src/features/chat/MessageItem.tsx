import { CornerUpLeft, Ellipsis, Pencil, SmilePlus, Trash2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { memo, useMemo, useState } from "react";
import { api, type Message } from "../../lib/api";
import { ChatMarkdown, pageIdsIn, plainPreview } from "../../lib/markdown";
import { displayName, formatTime } from "../../lib/people";
import { reportError } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, Badge, ConfirmDialog, cx, IconButton, Menu, type MenuItem, Popover, springs } from "../../ui";
import { MessageEditor } from "./MessageEditor";
import { PageCard } from "./PageCard";

const QUICK_REACTIONS = ["👍", "❤️", "😂", "🎉", "👀", "🚀"];

export const MessageItem = memo(function MessageItem({
  message,
  isContinuation,
  repliedTo,
  isHighlighted,
  onReply,
  onJumpTo,
}: {
  message: Message;
  isContinuation: boolean;
  repliedTo: Message | undefined;
  isHighlighted: boolean;
  onReply: () => void;
  onJumpTo: (id: string) => void;
}) {
  const { pages, navigate } = useWorkspace();
  const [isPickerOpen, setPickerOpen] = useState(false);
  const [isEditing, setEditing] = useState(false);
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const linkedPages = useMemo(() => pageIdsIn(message.text).filter((id) => titles.has(id)), [message.text, titles]);
  const name = displayName(message.author_name, message.author);

  const toggleReaction = (emoji: string) => {
    const isMine = message.reactions.find((reaction) => reaction.emoji === emoji)?.mine ?? false;
    api.react(message.id, emoji, !isMine).catch(reportError);
    setPickerOpen(false);
  };

  return (
    <article
      id={`message-${message.id}`}
      aria-label={`${name} at ${formatTime(message.created_ms)}`}
      className={cx(
        "group relative flex gap-3 px-5 transition-colors duration-500 hover:bg-surface-2/50 focus-within:bg-surface-2/50",
        isContinuation ? "py-0.5" : "pt-2 pb-0.5",
        isHighlighted && "bg-accent-soft/70",
      )}
    >
      <div className="w-9 shrink-0">
        {isContinuation ? (
          <span className="block pt-[3px] text-right text-[10.5px] text-faint opacity-0 group-hover:opacity-100">
            {formatTime(message.created_ms)}
          </span>
        ) : (
          <Avatar id={message.author} name={name} isBot={message.author_is_bot} size={36} />
        )}
      </div>

      <div className="min-w-0 flex-1">
        {repliedTo && (
          <button
            type="button"
            onClick={() => onJumpTo(repliedTo.id)}
            className="mb-0.5 flex max-w-full items-center gap-1.5 text-[12px] text-muted hover:text-ink"
          >
            <CornerUpLeft size={12} className="shrink-0" />
            <span className="font-medium">{displayName(repliedTo.author_name, repliedTo.author)}</span>
            <span className="truncate text-faint">{plainPreview(repliedTo.text, (id) => titles.get(id))}</span>
          </button>
        )}
        {!isContinuation && (
          <div className="flex items-baseline gap-2">
            <span className="text-[13.5px] font-semibold">{name}</span>
            {message.author_is_bot && <Badge tone="accent">bot</Badge>}
            <span className="text-[11.5px] text-faint">{formatTime(message.created_ms)}</span>
          </div>
        )}
        {isEditing ? (
          <MessageEditor
            initialText={message.text}
            onSave={(text) => api.editMessage(message.id, text)}
            onClose={() => setEditing(false)}
          />
        ) : (
          <div className="prose-chat selectable text-[14px]">
            <ChatMarkdown
              text={message.text}
              pageTitle={(id) => titles.get(id)}
              onOpenPage={(id) => navigate({ kind: "page", id })}
            />
            {message.is_edited && <span className="text-[11px] text-faint">(edited)</span>}
          </div>
        )}
        {linkedPages.map((id) => (
          <PageCard key={id} pageId={id} />
        ))}
        {message.reactions.length > 0 && (
          <motion.div layout className="mt-1 flex flex-wrap gap-1">
            <AnimatePresence initial={false}>
              {message.reactions.map((reaction) => (
                <motion.button
                  key={reaction.emoji}
                  type="button"
                  layout
                  aria-pressed={reaction.mine}
                  aria-label={`${reaction.emoji} ${reaction.count}`}
                  initial={{ scale: 0.4, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  exit={{ scale: 0.4, opacity: 0 }}
                  whileTap={{ scale: 0.88 }}
                  transition={springs.pop}
                  onClick={() => toggleReaction(reaction.emoji)}
                  className={cx(
                    "flex h-6 items-center gap-1 rounded-full border px-2 text-[12px] transition-colors",
                    reaction.mine
                      ? "border-accent/40 bg-accent-soft text-accent-text"
                      : "border-line bg-surface hover:border-surface-3",
                  )}
                >
                  <span>{reaction.emoji}</span>
                  <motion.span
                    key={reaction.count}
                    initial={{ y: -6, opacity: 0 }}
                    animate={{ y: 0, opacity: 1 }}
                    className="tabular-nums"
                  >
                    {reaction.count}
                  </motion.span>
                </motion.button>
              ))}
            </AnimatePresence>
          </motion.div>
        )}
      </div>

      <div
        className={cx(
          "absolute -top-3 right-5 flex items-center gap-0.5 rounded-lg border border-line bg-surface p-0.5 shadow-card transition",
          isPickerOpen ? "opacity-100" : "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100",
        )}
      >
        <Popover
          open={isPickerOpen}
          onOpenChange={setPickerOpen}
          trigger={<IconButton label="React" icon={<SmilePlus size={15} />} />}
        >
          <div className="flex gap-0.5">
            {QUICK_REACTIONS.map((emoji, index) => (
              <motion.button
                key={emoji}
                type="button"
                aria-label={`React with ${emoji}`}
                initial={{ opacity: 0, scale: 0.5 }}
                animate={{ opacity: 1, scale: 1, transition: { ...springs.pop, delay: index * 0.025 } }}
                whileHover={{ scale: 1.25 }}
                onClick={() => toggleReaction(emoji)}
                className="grid h-8 w-8 place-items-center rounded-md text-[17px] hover:bg-surface-2"
              >
                {emoji}
              </motion.button>
            ))}
          </div>
        </Popover>
        <IconButton label="Reply" icon={<CornerUpLeft size={15} />} onClick={onReply} />
        <MessageMenu message={message} onEdit={() => setEditing(true)} onDelete={() => setConfirmingDelete(true)} />
      </div>
      <ConfirmDialog
        open={isConfirmingDelete}
        onOpenChange={setConfirmingDelete}
        title="Delete this message?"
        description="It disappears for everyone, along with its reactions. This can't be undone."
        confirmLabel="Delete message"
        onConfirm={() => api.deleteMessage(message.id)}
      />
    </article>
  );
});

function MessageMenu({ message, onEdit, onDelete }: { message: Message; onEdit: () => void; onDelete: () => void }) {
  const items: MenuItem[] = [];
  if (message.can_edit) items.push({ label: "Edit", icon: <Pencil size={14} />, onSelect: onEdit });
  if (message.can_delete) {
    items.push({ label: "Delete", icon: <Trash2 size={14} />, onSelect: onDelete, isDestructive: true });
  }
  if (items.length === 0) return null;
  return <Menu trigger={<IconButton label="More actions" icon={<Ellipsis size={15} />} />} items={items} />;
}
