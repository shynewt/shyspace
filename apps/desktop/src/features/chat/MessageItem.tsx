import {
  Bookmark,
  BookmarkMinus,
  Copy,
  CornerUpLeft,
  Ellipsis,
  Link2,
  MessageSquareReply,
  Pencil,
  Plus,
  SmilePlus,
  Trash2,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { memo, useCallback, useMemo, useRef, useState } from "react";
import { api, type Message, messageLink, pageLink } from "../../lib/api";
import { ChatMarkdown, messageIdsIn, pageIdsIn, plainPreview } from "../../lib/markdown";
import { useMember } from "../../lib/members";
import { displayName, formatFullDate, formatTime } from "../../lib/people";
import { reportError, showToast } from "../../lib/toast";
import { useContextMenu } from "../../lib/useContextMenu";
import { linkedMessageView, useWorkspace } from "../../lib/workspace";
import {
  Badge,
  ConfirmDialog,
  cx,
  EmojiPicker,
  IconButton,
  Menu,
  type MenuItem,
  Popover,
  springs,
  Tooltip,
  tweens,
} from "../../ui";
import { ReminderPicker } from "../later/ReminderPicker";
import { remindMeItem } from "../later/remindMeItem";
import { MemberAvatar } from "../members/MemberAvatar";
import { ProfileCard, ProfileCardAvatar, ProfileCardTrigger } from "../members/ProfileCard";
import { StatusEmoji } from "../members/StatusEmoji";
import { MessageCard } from "./MessageCard";
import { MessageFiles } from "./MessageFiles";
import { PageCard } from "./PageCard";
import { describeSaved, SavedLabel } from "./SavedLabel";
import { ThreadSummary } from "./ThreadSummary";

const QUICK_REACTIONS = ["👍", "❤️", "😂", "🎉", "👀", "🚀"];
/** Each card loads its own data, so one message can't make the app load thousands; the rest stay links in the text. */
const MAX_CARDS_PER_MESSAGE = 3;

export const MessageItem = memo(function MessageItem({
  message,
  isContinuation,
  isHighlighted,
  onOpenThread,
  onEdit,
  onMention,
}: {
  message: Message;
  isContinuation: boolean;
  isHighlighted: boolean;
  /** Opens this message's thread; `null` inside a thread. */
  onOpenThread: (() => void) | null;
  /** Starts editing this message in the composer. */
  onEdit: () => void;
  /** Mentions a member in the message box beside this message. */
  onMention: (name: string, memberId: string) => void;
}) {
  const { pages, navigate, session } = useWorkspace();
  const [isPickerOpen, setPickerOpen] = useState(false);
  const [isFullPickerShown, setFullPickerShown] = useState(false);
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
  const [isMenuOpen, setMenuOpen] = useState(false);
  const article = useRef<HTMLElement>(null);
  const openMenu = useCallback(() => setMenuOpen(true), []);
  useContextMenu(article, openMenu);
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const linkedMessages = useMemo(
    () =>
      messageIdsIn(message.text)
        .filter((id) => id !== message.id)
        .slice(0, MAX_CARDS_PER_MESSAGE),
    [message],
  );
  const linkedPages = useMemo(
    () =>
      pageIdsIn(message.text)
        .filter((id) => titles.has(id))
        .slice(0, MAX_CARDS_PER_MESSAGE),
    [message.text, titles],
  );
  const cardLinks = useMemo(
    () => new Set([...linkedPages.map(pageLink), ...linkedMessages.map(messageLink)]),
    [linkedPages, linkedMessages],
  );
  const name = displayName(message.author_name, message.author);
  const author = useMember(message.author);
  const savedDescription = describeSaved(message.saved);

  const openLinkedMessage = (id: string) =>
    api.linkedMessage(id).then((linked) => {
      if (!linked) return showToast("That message was deleted.", "info");
      navigate(linkedMessageView(linked));
    }, reportError);

  const toggleReaction = (emoji: string) => {
    const isMine = message.reactions.find((reaction) => reaction.emoji === emoji)?.mine ?? false;
    api.react(message.id, emoji, !isMine).catch(reportError);
    setPickerOpen(false);
    setFullPickerShown(false);
  };

  return (
    <ProfileCard memberId={message.author} onMention={onMention}>
      <article
        ref={article}
        id={`message-${message.id}`}
        aria-label={`${name}, ${formatFullDate(message.created_ms)}${savedDescription ? `, ${savedDescription}` : ""}`}
        className={cx(
          "group relative isolate flex gap-3 px-5 hover:bg-surface-2/50 keyboard-focus-within:bg-surface-2/50",
          isContinuation ? "py-0.5" : "pt-2 pb-0.5",
          message.mentions_me && !isHighlighted && "bg-warning/8 shadow-[inset_2px_0_0_var(--warning)]",
          savedDescription && !message.mentions_me && !isHighlighted && "bg-accent-soft/35",
        )}
      >
        {/* Its own layer, so the highlight fades slowly while hover shows at once. */}
        <span
          aria-hidden
          className={cx(
            "pointer-events-none absolute inset-0 -z-10 bg-accent-soft/70 transition-opacity duration-highlight",
            !isHighlighted && "opacity-0",
          )}
        />
        <div className="w-9 shrink-0">
          {isContinuation ? (
            <span className="flex justify-end pt-[3px]">
              <MessageTime
                ms={message.created_ms}
                className="text-2xs whitespace-nowrap text-faint opacity-0 group-hover:opacity-100"
              />
            </span>
          ) : (
            <ProfileCardAvatar>
              <MemberAvatar id={message.author} name={name} isBot={message.author_is_bot} size={36} />
            </ProfileCardAvatar>
          )}
        </div>

        <div className="min-w-0 flex-1">
          {onOpenThread && message.reply_to && (
            // In the channel, a reply only shows when what it answered is gone.
            <div className="mb-0.5 flex items-center gap-1.5 text-xs text-faint">
              <CornerUpLeft size={12} className="shrink-0" /> Reply to a deleted message
            </div>
          )}
          {!isContinuation && (
            <div className="flex items-baseline gap-2">
              <ProfileCardTrigger opensOnHover className="rounded text-base font-semibold hover:underline">
                {name}
              </ProfileCardTrigger>
              {author && <StatusEmoji member={author} />}
              {message.author_is_bot && <Badge tone="accent">Agent</Badge>}
              <MessageTime ms={message.created_ms} className="text-xs text-faint" />
            </div>
          )}
          <SavedLabel saved={message.saved} />
          {(message.text !== "" || message.files.length === 0) && (
            <div className="prose-chat selectable text-base">
              <ChatMarkdown
                text={message.text}
                myId={session.me}
                pageTitle={(id) => titles.get(id)}
                onOpenPage={(id) => navigate({ kind: "page", id })}
                onOpenMessage={openLinkedMessage}
                cardLinks={cardLinks}
                renderMemberMention={(memberId, label, className) => (
                  <ProfileCard memberId={memberId} onMention={onMention}>
                    <ProfileCardTrigger className={className}>{label}</ProfileCardTrigger>
                  </ProfileCard>
                )}
              />
              {message.is_edited && <span className="edited-mark ml-1 text-2xs text-faint">(edited)</span>}
            </div>
          )}
          <MessageFiles files={message.files} />

          {linkedPages.map((id) => (
            <PageCard key={id} pageId={id} />
          ))}
          {linkedMessages.map((id) => (
            <MessageCard key={id} messageId={id} />
          ))}
          {message.reactions.length > 0 && (
            <div className="mt-1 flex flex-wrap gap-1">
              <AnimatePresence initial={false}>
                {message.reactions.map((reaction) => (
                  <motion.button
                    key={reaction.emoji}
                    type="button"
                    aria-pressed={reaction.mine}
                    aria-label={`${reaction.emoji} ${reaction.count}`}
                    initial={{ scale: 0.4, opacity: 0 }}
                    animate={{ scale: 1, opacity: 1 }}
                    whileTap={{ scale: 0.88 }}
                    transition={springs.pop}
                    onClick={() => toggleReaction(reaction.emoji)}
                    className={cx(
                      "flex h-6 items-center gap-1 rounded-full border px-2 text-xs transition-colors",
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
            </div>
          )}
          {onOpenThread && message.reply_count > 0 && (
            <ThreadSummary message={message} pageTitle={(id) => titles.get(id)} onOpen={onOpenThread} />
          )}
        </div>

        <div
          className={cx(
            "absolute -top-3 right-5 flex items-center gap-0.5 rounded-lg border border-line bg-surface p-0.5 shadow-card transition",
            isPickerOpen || isMenuOpen
              ? "opacity-100"
              : "opacity-0 group-hover:opacity-100 keyboard-focus-within:opacity-100",
          )}
        >
          <Popover
            open={isPickerOpen}
            onOpenChange={(open) => {
              setPickerOpen(open);
              if (!open) setFullPickerShown(false);
            }}
            trigger={<IconButton label="React" icon={<SmilePlus size={15} />} />}
          >
            {isFullPickerShown ? (
              <EmojiPicker onSelect={toggleReaction} />
            ) : (
              <div className="flex items-center gap-0.5">
                {QUICK_REACTIONS.map((emoji) => (
                  <motion.button
                    key={emoji}
                    type="button"
                    aria-label={`React with ${emoji}`}
                    whileHover={{ scale: 1.25 }}
                    transition={tweens.fast}
                    onClick={() => toggleReaction(emoji)}
                    className="grid h-8 w-8 place-items-center rounded-md text-xl hover:bg-surface-2"
                  >
                    {emoji}
                  </motion.button>
                ))}
                <IconButton
                  label="More emoji…"
                  icon={<Plus size={15} />}
                  onClick={() => setFullPickerShown(true)}
                  className="ml-0.5 h-8 w-8"
                />
              </div>
            )}
          </Popover>
          {onOpenThread && (
            <IconButton label="Reply in thread" icon={<MessageSquareReply size={15} />} onClick={onOpenThread} />
          )}
          <MessageMenu
            message={message}
            open={isMenuOpen}
            onOpenChange={setMenuOpen}
            onEdit={onEdit}
            onDelete={() => setConfirmingDelete(true)}
          />
        </div>
        <ConfirmDialog
          open={isConfirmingDelete}
          onOpenChange={setConfirmingDelete}
          title="Delete this message?"
          description={
            message.files.length > 0
              ? "It disappears for everyone, along with its reactions and files. This can't be undone."
              : "It disappears for everyone, along with its reactions. This can't be undone."
          }
          confirmLabel="Delete message"
          onConfirm={() => api.deleteMessage(message.id)}
        >
          {isConfirmingDelete && (
            <blockquote className="selectable line-clamp-3 border-l-3 border-surface-3 pl-2.5 text-sm text-ink">
              {plainPreview(message.text, (id) => titles.get(id))}
            </blockquote>
          )}
        </ConfirmDialog>
      </article>
    </ProfileCard>
  );
});

function MessageMenu({
  message,
  open,
  onOpenChange,
  onEdit,
  onDelete,
}: {
  message: Message;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [isPicking, setPicking] = useState(false);
  const remindMs = message.saved && !message.saved.is_done ? message.saved.remind_ms : null;
  const remind = (at: number | null) => api.saveForLater(message.id, at).catch(reportError);
  const copy = (text: string, done: string) =>
    navigator.clipboard.writeText(text).then(() => showToast(done), reportError);
  const items: MenuItem[] = [
    {
      label: "Copy text",
      icon: <Copy size={14} />,
      onSelect: () => copy(message.text, "Copied the message's markdown."),
    },
    {
      label: "Copy link",
      icon: <Link2 size={14} />,
      onSelect: () => copy(messageLink(message.id), "Link copied. Paste it in any channel to share this message."),
    },
  ];
  items.push(
    message.saved
      ? {
          label: "Remove from Later",
          icon: <BookmarkMinus size={14} />,
          onSelect: () => api.removeFromLater(message.id).catch(reportError),
        }
      : {
          label: "Save for later",
          icon: <Bookmark size={14} />,
          onSelect: () => api.saveForLater(message.id, null).catch(reportError),
        },
    remindMeItem({
      remindMs: remindMs,
      onRemind: remind,
      onCustom: () => setPicking(true),
    }),
  );
  if (message.can_edit) items.push({ label: "Edit", icon: <Pencil size={14} />, onSelect: onEdit });
  if (message.can_delete) {
    items.push({
      label: "Delete",
      icon: <Trash2 size={14} />,
      onSelect: onDelete,
      isDestructive: true,
    });
  }
  return (
    <>
      <Menu
        trigger={<IconButton label="More actions" icon={<Ellipsis size={15} />} />}
        items={items}
        open={open}
        onOpenChange={onOpenChange}
      />
      <ReminderPicker open={isPicking} onOpenChange={setPicking} initialMs={remindMs} onPick={remind} />
    </>
  );
}

function MessageTime({ ms, className }: { ms: number; className: string }) {
  return (
    <Tooltip label={formatFullDate(ms)}>
      <time dateTime={new Date(ms).toISOString()} className={className}>
        {formatTime(ms)}
      </time>
    </Tooltip>
  );
}
