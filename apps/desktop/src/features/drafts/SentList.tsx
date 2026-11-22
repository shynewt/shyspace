import {
  Bookmark,
  BookmarkMinus,
  Ellipsis,
  Link2,
  MessageSquareReply,
  Search,
  Send,
  SquareArrowOutUpRight,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type LinkedMessage, type Message, messageLink } from "../../lib/api";
import { plainPreview } from "../../lib/markdown";
import { formatTime, groupByDay } from "../../lib/people";
import { reportError, showToast } from "../../lib/toast";
import { useContextMenu } from "../../lib/useContextMenu";
import { useLive } from "../../lib/useLive";
import { linkedMessageView, useWorkspace } from "../../lib/workspace";
import { Button, cx, DayDivider, EmptyState, IconButton, Input, ListRow, Menu, type MenuItem } from "../../ui";
import { ReplierAvatars } from "../chat/ReplierAvatars";
import { ChannelName } from "../private/ChannelName";
import { isAwaitingReply } from "./responses";

const PAGE_SIZE = 50;
const FILTER_DELAY_MS = 150;

/** Your messages, newest first, grouped by day, with how people responded. */
export function SentList() {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("");
  const [limit, setLimit] = useState(PAGE_SIZE);
  useEffect(() => {
    const timer = setTimeout(() => {
      setFilter(query.trim());
      setLimit(PAGE_SIZE);
    }, FILTER_DELAY_MS);
    return () => clearTimeout(timer);
  }, [query]);
  // Everything shown reloads on a change, so replies and reactions stay current further down too.
  const load = useCallback(() => api.sentMessages(filter, null, limit), [filter, limit]);
  const sent = useLive(load, (change) => change.kind === "messages");
  const days = useMemo(() => groupByDay(sent ?? [], (item) => item.message.created_ms), [sent]);

  if (!sent) return null;
  if (sent.length === 0 && filter === "" && query === "") {
    return (
      <EmptyState icon={<Send size={22} />} title="Nothing sent yet">
        Messages you send show up here, with how people responded.
      </EmptyState>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      <div className="relative">
        <Search size={14} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-decorative" />
        <Input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Filter sent messages"
          aria-label="Filter sent messages"
          className="pl-8"
        />
      </div>
      {sent.length === 0 && filter !== "" && (
        <p className="px-2 py-6 text-center text-sm text-muted">Nothing you sent matches “{filter}”.</p>
      )}
      {days.map((day) => (
        <section key={day.label} aria-label={day.label}>
          <DayDivider label={day.label} />
          <ul className="flex flex-col gap-1">
            {day.items.map((item) => (
              <SentRow key={item.message.id} item={item} />
            ))}
          </ul>
        </section>
      ))}
      {sent.length === limit && (
        <Button variant="ghost" size="sm" className="self-center" onClick={() => setLimit(limit + PAGE_SIZE)}>
          Show older messages
        </Button>
      )}
    </div>
  );
}

function SentRow({ item }: { item: LinkedMessage }) {
  const { navigate, pages, session } = useWorkspace();
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const { message, thread_root: threadRoot } = item;
  const row = useRef<HTMLLIElement>(null);
  const [isMenuOpen, setMenuOpen] = useState(false);
  const openMenu = useCallback(() => setMenuOpen(true), []);
  useContextMenu(row, openMenu);

  const open = () => navigate(linkedMessageView(item));
  const copyLink = () =>
    navigator.clipboard
      .writeText(messageLink(message.id))
      .then(() => showToast("Link copied. Paste it in any channel to share this message."), reportError);
  const menu: MenuItem[] = [
    { label: "Open in context", icon: <SquareArrowOutUpRight size={14} />, onSelect: open },
    { label: "Copy link", icon: <Link2 size={14} />, onSelect: copyLink },
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
  ];
  const isAwaiting = isAwaitingReply(message, threadRoot, session.me);
  const hasResponses = message.reply_count > 0 || message.reactions.length > 0;

  return (
    <li ref={row}>
      <ListRow
        onClick={open}
        isHighlighted={isMenuOpen}
        leading={
          threadRoot ? (
            <MessageSquareReply size={15} className="mt-0.5 text-decorative" />
          ) : (
            <Send size={15} className="mt-0.5 text-decorative" />
          )
        }
        meta={
          <>
            {threadRoot && <span className="shrink-0">Thread in</span>}
            <ChannelName channelId={message.channel} className="min-w-0 font-semibold text-ink" />
            <time dateTime={new Date(message.created_ms).toISOString()} className="ml-auto shrink-0 text-faint">
              {formatTime(message.created_ms)}
            </time>
          </>
        }
        footer={
          hasResponses || isAwaiting ? (
            <>
              <Responses message={message} />
              {isAwaiting && <span>No replies yet</span>}
            </>
          ) : undefined
        }
        trailing={
          <span
            className={cx(
              "flex shrink-0 opacity-0 transition-opacity group-hover:opacity-100 keyboard-focus-within:opacity-100",
              isMenuOpen && "opacity-100",
            )}
          >
            <Menu
              open={isMenuOpen}
              onOpenChange={setMenuOpen}
              trigger={<IconButton label="Sent message actions" size="sm" icon={<Ellipsis size={14} />} />}
              items={menu}
            />
          </span>
        }
      >
        {plainPreview(message.text, (id) => titles.get(id))}
      </ListRow>
    </li>
  );
}

/** The thread's repliers and count, then each reaction with its count. */
function Responses({ message }: { message: Message }) {
  const count = message.reply_count;
  return (
    <>
      {count > 0 && (
        <span className="flex items-center gap-1.5">
          <ReplierAvatars
            authorIds={message.recent_reply_authors}
            ringClassName="ring-surface group-hover:ring-surface-2"
          />
          <span className="font-medium whitespace-nowrap text-accent-text">
            {count} {count === 1 ? "reply" : "replies"}
          </span>
        </span>
      )}
      {message.reactions.map((reaction) => (
        <span
          key={reaction.emoji}
          className="inline-flex items-center gap-1 rounded-full border border-line px-1.5 text-xs tabular-nums"
        >
          {reaction.emoji} {reaction.count}
        </span>
      ))}
    </>
  );
}
