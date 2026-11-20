import { Bot, Hash, Lock } from "lucide-react";
import { AnimatePresence, motion, useIsPresent } from "motion/react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, type Message, type MessageCursor } from "../../lib/api";
import { channelReference, channelTitle, composerLabel } from "../../lib/channels";
import { formatDay } from "../../lib/people";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { EmptyState, Spinner, springs, Tooltip } from "../../ui";
import { ChannelTitle } from "../private/ChannelTitle";
import { WriteGate } from "../private/WriteGate";
import { Composer } from "./Composer";
import { MessageItem } from "./MessageItem";
import { ThreadPanel } from "./ThreadPanel";

const PAGE_SIZE = 100;
const GROUP_WINDOW_MS = 5 * 60 * 1000;
const NEAR_EDGE_PX = 80;
const HIGHLIGHT_MS = 1600;

type Row =
  | { kind: "day"; key: string; label: string }
  | { kind: "unread"; key: string }
  | { kind: "message"; key: string; message: Message; isContinuation: boolean };

function isAfter(message: Message, cursor: MessageCursor): boolean {
  return message.lamport > cursor.lamport || (message.lamport === cursor.lamport && message.author > cursor.author);
}

/** The first message by someone else after the read position. */
function firstUnreadId(messages: Message[], lastRead: MessageCursor | null, me: string): string | null {
  const first = messages.find((message) => message.author !== me && (!lastRead || isAfter(message, lastRead)));
  return first?.id ?? null;
}

function toRows(messages: Message[], unreadFrom: string | null): Row[] {
  const rows: Row[] = [];
  let previous: Message | undefined;
  for (const message of messages) {
    const day = formatDay(message.created_ms);
    if (!previous || formatDay(previous.created_ms) !== day) rows.push({ kind: "day", key: `day:${day}`, label: day });
    if (message.id === unreadFrom) rows.push({ kind: "unread", key: "unread" });
    const isContinuation =
      !!previous &&
      previous.author === message.author &&
      !message.reply_to &&
      message.created_ms - previous.created_ms < GROUP_WINDOW_MS &&
      rows[rows.length - 1].kind === "message";
    rows.push({ kind: "message", key: message.id, message, isContinuation });
    previous = message;
  }
  return rows;
}

export function ChannelView({ channelId, thread, focus }: { channelId: string; thread?: string; focus?: string }) {
  const { channels, session } = useWorkspace();
  const channel = channels.find((candidate) => candidate.id === channelId);
  const members = useLive(api.members, (change) => change.kind === "members");
  // Where you left off, as of opening the channel: the line stays put while
  // you read, until Escape dismisses it.
  const [lastReadOnOpen] = useState(() => channel?.last_read ?? null);
  const [isUnreadLineDismissed, setUnreadLineDismissed] = useState(false);
  // Loading older messages grows the window, so new arrivals never leave gaps.
  const [windowSize, setWindowSize] = useState(PAGE_SIZE);
  const loadMessages = useCallback(() => api.messages(channelId, null, windowSize), [channelId, windowSize]);
  const messages = useLive(
    loadMessages,
    (change) => (change.kind === "messages" && change.channel === channelId) || change.kind === "members",
  );
  const hasOlder = (messages?.length ?? 0) >= windowSize;
  const [openThread, setOpenThread] = useState<string | null>(thread ?? null);
  useEffect(() => {
    if (thread) setOpenThread(thread);
  }, [thread]);
  const [highlighted, setHighlighted] = useState<string | null>(null);
  const [editing, setEditing] = useState<Message | null>(null);
  // A linked message to show: in the channel (loading older history until
  // it's there), or in the open thread, which handles it.
  const [pendingFocus, setPendingFocus] = useState(thread ? null : (focus ?? null));
  useEffect(() => {
    if (focus && !thread) setPendingFocus(focus);
  }, [focus, thread]);
  const unreadFrom = useMemo(
    () => (isUnreadLineDismissed ? null : firstUnreadId(messages ?? [], lastReadOnOpen, session.me)),
    [messages, lastReadOnOpen, session.me, isUnreadLineDismissed],
  );
  const rows = useMemo(() => toRows(messages ?? [], unreadFrom), [messages, unreadFrom]);
  const hasScrolledToUnread = useRef(false);
  const scroller = useRef<HTMLDivElement>(null);
  const isPinnedToBottom = useRef(true);
  const heightBeforeLoadingOlder = useRef<number | null>(null);

  // Keeps the view anchored whenever the rows change.
  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element || rows.length === 0) return;
    if (heightBeforeLoadingOlder.current !== null) {
      element.scrollTop += element.scrollHeight - heightBeforeLoadingOlder.current;
      heightBeforeLoadingOlder.current = null;
    } else if (!hasScrolledToUnread.current && unreadFrom) {
      hasScrolledToUnread.current = true;
      document.getElementById("unread-line")?.scrollIntoView({ block: "start" });
      isPinnedToBottom.current = element.scrollHeight - element.scrollTop - element.clientHeight < NEAR_EDGE_PX;
    } else if (isPinnedToBottom.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [rows, unreadFrom]);

  useEffect(() => {
    // Escape that closed something else, like a menu or the palette, isn't for the line.
    const dismissOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) setUnreadLineDismissed(true);
    };
    window.addEventListener("keydown", dismissOnEscape);
    return () => window.removeEventListener("keydown", dismissOnEscape);
  }, []);

  // What's on screen while the window has focus counts as read; not while
  // this view is fading out after you switched away.
  const latestId = messages?.at(-1)?.id;
  const isPresent = useIsPresent();
  useEffect(() => {
    if (!latestId || !isPresent) return;
    const markRead = () => {
      if (document.hasFocus()) api.markRead(channelId, latestId).catch(reportError);
    };
    markRead();
    window.addEventListener("focus", markRead);
    return () => window.removeEventListener("focus", markRead);
  }, [channelId, latestId, isPresent]);

  useEffect(() => {
    if (!pendingFocus || !messages) return;
    const target = pendingFocus;
    if (messages.some((message) => message.id === target)) {
      setPendingFocus(null);
      hasScrolledToUnread.current = true;
      isPinnedToBottom.current = false;
      document.getElementById(`message-${target}`)?.scrollIntoView({ block: "center" });
      setHighlighted(target);
    } else if (hasOlder) {
      setWindowSize((size) => size + PAGE_SIZE);
    } else {
      setPendingFocus(null);
    }
  }, [messages, hasOlder, pendingFocus]);

  useEffect(() => {
    if (!highlighted) return;
    const timer = setTimeout(() => setHighlighted(null), HIGHLIGHT_MS);
    return () => clearTimeout(timer);
  }, [highlighted]);

  const loadOlder = () => {
    if (!hasOlder || heightBeforeLoadingOlder.current !== null || !scroller.current) return;
    heightBeforeLoadingOlder.current = scroller.current.scrollHeight;
    setWindowSize((size) => size + PAGE_SIZE);
  };

  return (
    <div className="flex h-full">
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-13 shrink-0 items-center gap-2 border-b border-line px-5">
          {channel ? <ChannelTitle channel={channel} members={members} /> : <h1 className="text-[15px]">…</h1>}
          <AgentsNote />
          <span className="flex items-center gap-1.5 rounded-full bg-surface-2 px-2.5 py-1 text-[11.5px] text-muted">
            <Lock size={11} /> End-to-end encrypted
          </span>
        </header>

        <div
          ref={scroller}
          role="log"
          aria-label={`Messages in ${channelReference(channel, members)}`}
          onScroll={(event) => {
            const element = event.currentTarget;
            isPinnedToBottom.current = element.scrollHeight - element.scrollTop - element.clientHeight < NEAR_EDGE_PX;
            if (element.scrollTop < NEAR_EDGE_PX) loadOlder();
          }}
          className="flex-1 overflow-y-auto py-3"
        >
          {hasOlder && (
            <div className="flex justify-center py-2 text-decorative">
              <Spinner size={14} />
            </div>
          )}
          {messages && messages.length === 0 && (
            <EmptyState
              icon={channel && channel.kind !== "public" ? <Lock size={22} /> : <Hash size={22} />}
              title={`This is the start of ${channel?.kind === "dm" ? "your conversation with " : "#"}${channel ? channelTitle(channel, members) : ""}`}
            >
              {!channel || channel.kind === "public"
                ? "Say hi. Only workspace members can read what you write."
                : "Only the people in it can read what you write, not even the workspace admin."}
            </EmptyState>
          )}
          <AnimatePresence initial={false}>
            {rows.map((row) =>
              row.kind === "day" ? (
                <DayDivider key={row.key} label={row.label} />
              ) : row.kind === "unread" ? (
                <UnreadLine key={row.key} />
              ) : (
                <motion.div
                  key={row.key}
                  layout="position"
                  initial={{ opacity: 0, y: 10, scale: 0.99 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  transition={springs.smooth}
                >
                  <MessageItem
                    message={row.message}
                    isContinuation={row.isContinuation}
                    isHighlighted={
                      openThread === row.message.id || highlighted === row.message.id || editing?.id === row.message.id
                    }
                    onOpenThread={() => setOpenThread(row.message.id)}
                    onEdit={() => setEditing(row.message)}
                  />
                </motion.div>
              ),
            )}
          </AnimatePresence>
        </div>

        <WriteGate channelId={channelId}>
          <Composer
            channelId={channelId}
            label={composerLabel(channel, members)}
            threadRoot={null}
            onSent={() => {
              isPinnedToBottom.current = true;
            }}
            editing={editing}
            onEditDone={() => setEditing(null)}
            onEditLast={() => setEditing(messages?.findLast((message) => message.can_edit) ?? null)}
          />
        </WriteGate>
      </div>
      <AnimatePresence>
        {openThread && (
          <ThreadPanel
            key={openThread}
            root={openThread}
            focus={openThread === thread ? focus : undefined}
            channelId={channelId}
            onClose={() => setOpenThread(null)}
          />
        )}
      </AnimatePresence>
    </div>
  );
}

/** Agents can read every channel; say so where people write. */
function AgentsNote() {
  const loadMembers = useCallback(() => api.members(), []);
  const members = useLive(loadMembers, (change) => change.kind === "members");
  const agents = members?.filter((member) => member.active && member.is_bot).length ?? 0;
  return (
    <span className="ml-auto">
      {agents > 0 && (
        <Tooltip
          label={
            <span className="block max-w-[260px] whitespace-normal">
              AI agents are members and can read this channel. What they read goes to their AI provider.
            </span>
          }
          side="bottom"
        >
          <span className="flex items-center gap-1.5 rounded-full bg-accent-soft px-2.5 py-1 text-[11.5px] text-accent-text">
            <Bot size={12} /> {agents} {agents === 1 ? "agent" : "agents"} can read this
          </span>
        </Tooltip>
      )}
    </span>
  );
}

function UnreadLine() {
  return (
    <motion.div
      id="unread-line"
      role="separator"
      aria-label="New messages"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="my-2 flex items-center gap-3 px-5"
    >
      <div className="h-px flex-1 bg-danger/60" />
      <span className="text-[11px] font-semibold tracking-wide text-danger-text uppercase">New messages</span>
    </motion.div>
  );
}

function DayDivider({ label }: { label: string }) {
  return (
    <div className="my-3 flex items-center gap-3 px-5">
      <div className="h-px flex-1 bg-line" />
      <span className="text-[11.5px] font-medium text-faint">{label}</span>
      <div className="h-px flex-1 bg-line" />
    </div>
  );
}
