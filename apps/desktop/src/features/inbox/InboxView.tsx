import { AtSign, CheckCheck, Hash, Inbox as InboxIcon, MessageSquareReply, Pencil } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useMemo } from "react";
import { api, type Channel, type Draft, type InboxItem } from "../../lib/api";
import { plainPreview } from "../../lib/markdown";
import { displayName, formatTime } from "../../lib/people";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, Button, EmptyState, SectionLabel, springs } from "../../ui";

/** Everything that needs your attention, across channels and threads. */
export function InboxView() {
  const loadInbox = useCallback(() => api.inbox(), []);
  const inbox = useLive(loadInbox, (change) => change.kind === "messages" || change.kind === "channels");
  const drafts = useLive(api.drafts, (change) => change.kind === "drafts") ?? [];
  const isEmpty = inbox && inbox.attention.length === 0 && inbox.channels.length === 0 && drafts.length === 0;

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-13 shrink-0 items-center gap-2 border-b border-line px-5">
        <InboxIcon size={17} className="text-faint" />
        <h1 className="text-[15px] font-semibold">Inbox</h1>
        {inbox && !isEmpty && (
          <Button
            variant="ghost"
            size="sm"
            icon={<CheckCheck size={14} />}
            onClick={() => api.markAllRead().catch(reportError)}
            className="ml-auto"
          >
            Mark all as read
          </Button>
        )}
      </header>
      <div className="flex-1 overflow-y-auto px-5 py-4">
        {isEmpty && (
          <EmptyState icon={<CheckCheck size={22} />} title="You're all caught up">
            Mentions of you and new replies in threads you follow show up here.
          </EmptyState>
        )}
        {inbox && inbox.attention.length > 0 && (
          <section aria-label="Mentions and replies" className="mb-6">
            <SectionLabel>Mentions and replies</SectionLabel>
            <ul className="flex flex-col gap-1">
              <AnimatePresence initial={false}>
                {inbox.attention.map((item) => (
                  <AttentionRow key={item.message.id} item={item} />
                ))}
              </AnimatePresence>
            </ul>
          </section>
        )}
        {drafts.length > 0 && (
          <section aria-label="Drafts" className="mb-6">
            <SectionLabel>Drafts</SectionLabel>
            <ul className="flex flex-col gap-1">
              <AnimatePresence initial={false}>
                {drafts.map((draft) => (
                  <DraftRow key={draft.target} draft={draft} />
                ))}
              </AnimatePresence>
            </ul>
          </section>
        )}
        {inbox && inbox.channels.length > 0 && (
          <section aria-label="Unread channels">
            <SectionLabel>Unread channels</SectionLabel>
            <ul className="flex flex-col gap-1">
              {inbox.channels.map((channel) => (
                <UnreadChannelRow key={channel.id} channel={channel} />
              ))}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}

function AttentionRow({ item }: { item: InboxItem }) {
  const { navigate, pages } = useWorkspace();
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const { message } = item;
  const name = displayName(message.author_name, message.author);
  const where =
    item.reason === "mention"
      ? `mentioned you in #${item.channel_name}`
      : `replied in a thread in #${item.channel_name}`;
  return (
    <motion.li
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0 }}
      transition={springs.smooth}
    >
      <button
        type="button"
        onClick={() => navigate({ kind: "channel", id: message.channel, thread: item.thread_root ?? undefined })}
        className="flex w-full items-start gap-3 rounded-xl px-3 py-2.5 text-left hover:bg-surface-2"
      >
        <Avatar id={message.author} name={name} isBot={message.author_is_bot} size={32} />
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-1.5 text-[12.5px] text-muted">
            {item.reason === "mention" ? <AtSign size={12} /> : <MessageSquareReply size={12} />}
            <b className="font-semibold text-ink">{name}</b> {where}
            <span className="ml-auto shrink-0 text-faint">{formatTime(message.created_ms)}</span>
          </span>
          <span className="mt-0.5 block truncate text-[13.5px]">
            {plainPreview(message.text, (id) => titles.get(id))}
          </span>
        </span>
      </button>
    </motion.li>
  );
}

function DraftRow({ draft }: { draft: Draft }) {
  const { channels, navigate } = useWorkspace();
  const channelName = channels.find((channel) => channel.id === draft.channel)?.name ?? "";
  const isThread = draft.target !== draft.channel;
  return (
    <motion.li
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0 }}
      transition={springs.smooth}
    >
      <button
        type="button"
        onClick={() => navigate({ kind: "channel", id: draft.channel, thread: isThread ? draft.target : undefined })}
        className="flex w-full items-start gap-3 rounded-xl px-3 py-2.5 text-left hover:bg-surface-2"
      >
        <Pencil size={15} className="mt-0.5 shrink-0 text-faint" />
        <span className="min-w-0 flex-1">
          <span className="block text-[12.5px] text-muted">
            {isThread ? "Reply in a thread in" : "In"} <b className="font-semibold text-ink">#{channelName}</b>
          </span>
          <span className="mt-0.5 block truncate text-[13.5px]">{draft.text}</span>
        </span>
      </button>
    </motion.li>
  );
}

function UnreadChannelRow({ channel }: { channel: Channel }) {
  const { navigate } = useWorkspace();
  const markRead = async () => {
    const [latest] = await api.messages(channel.id, null, 1);
    if (latest) await api.markRead(channel.id, latest.id);
  };
  return (
    <li className="flex items-center gap-2 rounded-xl px-3 py-1.5 hover:bg-surface-2">
      <button
        type="button"
        onClick={() => navigate({ kind: "channel", id: channel.id })}
        className="flex min-w-0 flex-1 items-center gap-2 text-left text-[13.5px] font-semibold"
      >
        <Hash size={15} className="text-faint" />
        <span className="truncate">{channel.name}</span>
        <span className="rounded-full bg-accent px-1.5 text-[10.5px] font-semibold text-white tabular-nums">
          {channel.unread}
        </span>
      </button>
      <Button variant="ghost" size="sm" onClick={() => markRead().catch(reportError)}>
        Mark read
      </Button>
    </li>
  );
}
