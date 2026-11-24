import { AtSign, CheckCheck, Inbox as InboxIcon, MessageSquareReply } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useMemo, useState } from "react";
import { api, type Channel, type InboxItem } from "../../lib/api";
import { plainPreview } from "../../lib/markdown";
import { useMemberNames } from "../../lib/members";
import { displayName } from "../../lib/people";
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
  const inbox = useLive(loadInbox, (change) => change.kind === "messages" || change.kind === "channels");
  const [isConfirmingMarkAll, setConfirmingMarkAll] = useState(false);
  // Like the sidebar: a muted channel needs you only when it mentions you.
  const unreadChannels =
    inbox?.channels.filter((channel) => channel.notify !== "nothing" || channel.unread_mentions > 0) ?? [];
  const isEmpty = inbox && inbox.attention.length === 0 && unreadChannels.length === 0;

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

function AttentionRow({ item }: { item: InboxItem }) {
  const { navigate, pages } = useWorkspace();
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
        {plainPreview(message.text, (id) => titles.get(id), memberName)}
      </ListRow>
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
        className="flex min-w-0 flex-1 items-center gap-2 text-left text-base font-semibold"
      >
        <ChannelName channelId={channel.id} className="min-w-0" />
        <CountBadge>{channel.unread}</CountBadge>
      </button>
      <Button variant="ghost" size="sm" onClick={() => markRead().catch(reportError)}>
        Mark read
      </Button>
    </li>
  );
}
