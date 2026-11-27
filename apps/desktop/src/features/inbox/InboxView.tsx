import { AtSign, CheckCheck, FileText, Inbox as InboxIcon, MessageSquareReply } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useMemo, useState } from "react";
import { api, type Change, type Channel, type InboxItem, type PageMention } from "../../lib/api";
import { useMember, useMemberNames } from "../../lib/members";
import { displayName } from "../../lib/people";
import { plainPreview } from "../../lib/plainPreview";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, CountBadge, EmptyState, ListRow, RelativeTime, SectionLabel, tweens } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";
import { ChannelName } from "../private/ChannelName";
import { HeaderAction, ViewHeader } from "../shell/ViewHeader";
import { MarkAllReadDialog } from "./MarkAllReadDialog";

/** Everything that needs your attention, across channels and threads. */
export function InboxView() {
  const loadInbox = useCallback(() => api.inbox(), []);
  const inbox = useLive(loadInbox, isInboxChange);
  const [isConfirmingMarkAll, setConfirmingMarkAll] = useState(false);
  // The core leaves out muted channels unless they mention you.
  const unreadChannels = inbox?.channels ?? [];
  const entries = inbox ? attentionEntries(inbox.attention, inbox.page_mentions) : [];
  const isEmpty = inbox && entries.length === 0 && unreadChannels.length === 0;

  return (
    <div className="flex h-full flex-col">
      <ViewHeader>
        <InboxIcon size={17} className="shrink-0 text-decorative" />
        <h1 className="truncate text-lg font-semibold">Inbox</h1>
        {inbox && !isEmpty && (
          <HeaderAction
            icon={<CheckCheck size={14} />}
            label="Mark all as read"
            onClick={() => setConfirmingMarkAll(true)}
            className="ml-auto"
          />
        )}
        <MarkAllReadDialog open={isConfirmingMarkAll} onOpenChange={setConfirmingMarkAll} />
      </ViewHeader>
      <div className="flex-1 overflow-y-auto px-5 py-4">
        <div className="mx-auto max-w-reading">
          {isEmpty && (
            <EmptyState icon={<CheckCheck size={22} />} title="You're all caught up">
              Mentions of you, in chat and in pages, and new replies in threads you follow show up here.
            </EmptyState>
          )}
          {entries.length > 0 && (
            <section aria-label="Mentions and replies" className="mb-6">
              <SectionLabel>Mentions and replies</SectionLabel>
              <ul className="flex flex-col gap-1">
                <AnimatePresence initial={false}>
                  {entries.map((entry) =>
                    entry.kind === "message" ? (
                      <AttentionRow key={entry.item.message.id} item={entry.item} />
                    ) : (
                      <PageMentionRow key={`${entry.mention.page}:${entry.mention.author}`} mention={entry.mention} />
                    ),
                  )}
                </AnimatePresence>
              </ul>
            </section>
          )}
          {unreadChannels.length > 0 && (
            <section aria-label="Unread channels">
              <SectionLabel>Unread channels</SectionLabel>
              <ul className="flex flex-col gap-1">
                {unreadChannels.map((channel) => (
                  <UnreadChannelRow key={channel.id} channel={channel} />
                ))}
              </ul>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}

/** What the inbox and its sidebar count reload on. */
export function isInboxChange(change: Change): boolean {
  return change.kind === "messages" || change.kind === "channels" || change.kind === "page_mentions";
}

type AttentionEntry =
  | { kind: "message"; item: InboxItem; ms: number }
  | { kind: "page"; mention: PageMention; ms: number };

/** Chat items and page mentions together, newest first. */
function attentionEntries(items: InboxItem[], mentions: PageMention[]): AttentionEntry[] {
  return [
    ...items.map((item): AttentionEntry => ({ kind: "message", item, ms: item.message.created_ms })),
    ...mentions.map((mention): AttentionEntry => ({ kind: "page", mention, ms: mention.mentioned_ms })),
  ].sort((left, right) => right.ms - left.ms);
}

function PageMentionRow({ mention }: { mention: PageMention }) {
  const { navigate } = useWorkspace();
  const author = useMember(mention.author);
  const name = displayName(author?.name ?? "", mention.author);
  return (
    <motion.li
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, transition: tweens.base }}
    >
      <ListRow
        onClick={() => navigate({ kind: "page", id: mention.page, at: mention.position })}
        leading={<MemberAvatar id={mention.author} name={name} isBot={author?.is_bot ?? false} size={32} />}
        meta={
          <>
            <AtSign size={12} />
            <b className="font-semibold text-ink">{name}</b> mentioned you in
            <span className="flex min-w-0 items-center gap-1">
              <FileText size={12} className="shrink-0" />
              <span className="truncate">{mention.page_title || "Untitled"}</span>
            </span>
            <RelativeTime ms={mention.mentioned_ms} className="ml-auto shrink-0 text-faint" />
          </>
        }
      >
        {mention.preview}
      </ListRow>
    </motion.li>
  );
}

function AttentionRow({ item }: { item: InboxItem }) {
  const { navigate, pages, session } = useWorkspace();
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const memberName = useMemberNames();
  const { message } = item;
  const name = displayName(message.author_name, message.author);
  const where = item.reason === "mention" ? "mentioned you in" : "replied in a thread in";
  return (
    <motion.li
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, transition: tweens.base }}
    >
      <ListRow
        onClick={() =>
          navigate({ kind: "channel", id: message.channel, thread: item.thread_root ?? undefined, focus: message.id })
        }
        leading={<MemberAvatar id={message.author} name={name} isBot={message.author_is_bot} size={32} />}
        meta={
          <>
            {item.reason === "mention" ? <AtSign size={12} /> : <MessageSquareReply size={12} />}
            <b className="font-semibold text-ink">{name}</b> {where}{" "}
            <ChannelName channelId={message.channel} className="min-w-0" />
            <RelativeTime ms={message.created_ms} className="ml-auto shrink-0 text-faint" />
          </>
        }
      >
        {plainPreview(message.text, (id) => titles.get(id), memberName, session.workspace_id)}
      </ListRow>
    </motion.li>
  );
}

function UnreadChannelRow({ channel }: { channel: Channel }) {
  const { navigate } = useWorkspace();
  return (
    <li className="flex items-center gap-2 rounded-xl px-3 py-1.5 hover:bg-surface-2">
      <button
        type="button"
        onClick={() => navigate({ kind: "channel", id: channel.id })}
        className="flex min-w-0 flex-1 items-center gap-2 text-left text-base font-semibold"
      >
        <ChannelName channelId={channel.id} className="min-w-0" />
        <CountBadge>{channel.unread}</CountBadge>
      </button>
      <Button variant="ghost" size="sm" onClick={() => api.markChannelRead(channel.id).catch(reportError)}>
        Mark read
      </Button>
    </li>
  );
}
