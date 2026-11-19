import { memo, useMemo, useState, type MouseEvent } from "react";
import { AnimatePresence, motion } from "motion/react";
import { CornerUpLeft, SmilePlus } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type Message } from "../lib/api";
import { pageIdsIn, plainPreview, renderMarkdown } from "../lib/markdown";
import { displayName, formatTime } from "../lib/people";
import { reportError } from "../lib/toast";
import { useWorkspace } from "../lib/workspace";
import { Avatar } from "./Avatar";
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
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const html = useMemo(() => renderMarkdown(message.text, (id) => titles.get(id)), [message.text, titles]);
  const linkedPages = useMemo(() => pageIdsIn(message.text).filter((id) => titles.has(id)), [message.text, titles]);
  const name = displayName(message.author_name, message.author);

  const toggleReaction = (emoji: string) => {
    const mine = message.reactions.find((reaction) => reaction.emoji === emoji)?.mine ?? false;
    api.react(message.id, emoji, !mine).catch(reportError);
    setPickerOpen(false);
  };

  const onContentClick = (event: MouseEvent) => {
    const anchor = (event.target as HTMLElement).closest("a");
    if (!anchor) return;
    event.preventDefault();
    const page = anchor.getAttribute("data-page");
    if (page) return navigate({ kind: "page", id: page });
    const href = anchor.getAttribute("href");
    if (href?.match(/^(https?:|mailto:)/)) openUrl(href).catch(reportError);
  };

  return (
    <div
      id={`message-${message.id}`}
      className={`group relative flex gap-3 px-5 transition-colors duration-500 hover:bg-surface-2/50 ${
        isContinuation ? "py-0.5" : "pt-2 pb-0.5"
      } ${isHighlighted ? "bg-accent-soft/70" : ""}`}
      onMouseLeave={() => setPickerOpen(false)}
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
            {message.author_is_bot && (
              <span className="rounded bg-accent-soft px-1 py-px text-[10px] font-semibold tracking-wide text-accent-text uppercase">
                bot
              </span>
            )}
            <span className="text-[11.5px] text-faint">{formatTime(message.created_ms)}</span>
          </div>
        )}
        <div className="prose-chat selectable text-[14px]" onClick={onContentClick} dangerouslySetInnerHTML={{ __html: html }} />
        {linkedPages.map((id) => (
          <PageCard key={id} pageId={id} />
        ))}
        {message.reactions.length > 0 && (
          <motion.div layout className="mt-1 flex flex-wrap gap-1">
            <AnimatePresence initial={false}>
              {message.reactions.map((reaction) => (
                <motion.button
                  key={reaction.emoji}
                  layout
                  initial={{ scale: 0.4, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  exit={{ scale: 0.4, opacity: 0 }}
                  whileTap={{ scale: 0.88 }}
                  onClick={() => toggleReaction(reaction.emoji)}
                  className={`flex h-6 items-center gap-1 rounded-full border px-2 text-[12px] transition-colors ${
                    reaction.mine
                      ? "border-accent/40 bg-accent-soft text-accent-text"
                      : "border-line bg-surface hover:border-surface-3"
                  }`}
                >
                  <span>{reaction.emoji}</span>
                  <motion.span key={reaction.count} initial={{ y: -6, opacity: 0 }} animate={{ y: 0, opacity: 1 }} className="tabular-nums">
                    {reaction.count}
                  </motion.span>
                </motion.button>
              ))}
            </AnimatePresence>
          </motion.div>
        )}
      </div>

      <div className="absolute -top-3 right-5 flex items-center gap-0.5 rounded-lg border border-line bg-surface p-0.5 opacity-0 shadow-card transition group-hover:opacity-100">
        <AnimatePresence>
          {isPickerOpen &&
            QUICK_REACTIONS.map((emoji, index) => (
              <motion.button
                key={emoji}
                initial={{ opacity: 0, scale: 0.5 }}
                animate={{ opacity: 1, scale: 1, transition: { delay: index * 0.025 } }}
                exit={{ opacity: 0, scale: 0.5, transition: { duration: 0.08 } }}
                whileHover={{ scale: 1.25 }}
                onClick={() => toggleReaction(emoji)}
                className="grid h-7 w-7 place-items-center rounded-md text-[15px]"
              >
                {emoji}
              </motion.button>
            ))}
        </AnimatePresence>
        <ToolbarButton label="React" onClick={() => setPickerOpen(!isPickerOpen)}>
          <SmilePlus size={15} />
        </ToolbarButton>
        <ToolbarButton label="Reply" onClick={onReply}>
          <CornerUpLeft size={15} />
        </ToolbarButton>
      </div>
    </div>
  );
});

function ToolbarButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      aria-label={label}
      title={label}
      className="grid h-7 w-7 place-items-center rounded-md text-muted transition hover:bg-surface-2 hover:text-ink"
    >
      {children}
    </button>
  );
}
