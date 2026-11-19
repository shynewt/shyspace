import { Hash, Lock } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, type Message } from "../../lib/api";
import { formatDay } from "../../lib/people";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { EmptyState, Spinner, springs } from "../../ui";
import { Composer } from "./Composer";
import { MessageItem } from "./MessageItem";

const PAGE_SIZE = 100;
const GROUP_WINDOW_MS = 5 * 60 * 1000;
const NEAR_EDGE_PX = 80;
const HIGHLIGHT_MS = 1600;

type Row =
  | { kind: "day"; key: string; label: string }
  | { kind: "message"; key: string; message: Message; isContinuation: boolean };

function toRows(messages: Message[]): Row[] {
  const rows: Row[] = [];
  let previous: Message | undefined;
  for (const message of messages) {
    const day = formatDay(message.created_ms);
    if (!previous || formatDay(previous.created_ms) !== day) rows.push({ kind: "day", key: `day:${day}`, label: day });
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

export function ChannelView({ channelId }: { channelId: string }) {
  const { channels } = useWorkspace();
  const channel = channels.find((candidate) => candidate.id === channelId);
  // Loading older messages grows the window, so new arrivals never leave gaps.
  const [windowSize, setWindowSize] = useState(PAGE_SIZE);
  const loadMessages = useCallback(() => api.messages(channelId, null, windowSize), [channelId, windowSize]);
  const messages = useLive(
    loadMessages,
    (change) => (change.kind === "messages" && change.channel === channelId) || change.kind === "members",
  );
  const hasOlder = (messages?.length ?? 0) >= windowSize;
  const [replyTo, setReplyTo] = useState<Message | null>(null);
  const [highlighted, setHighlighted] = useState<string | null>(null);
  const rows = useMemo(() => toRows(messages ?? []), [messages]);
  const byId = useMemo(() => new Map((messages ?? []).map((message) => [message.id, message])), [messages]);
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
    } else if (isPinnedToBottom.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [rows]);

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

  const jumpTo = (id: string) => {
    document.getElementById(`message-${id}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
    setHighlighted(id);
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-13 shrink-0 items-center gap-2 border-b border-line px-5">
        <Hash size={17} className="text-faint" />
        <h1 className="text-[15px] font-semibold">{channel?.name ?? "…"}</h1>
        <span className="ml-auto flex items-center gap-1.5 rounded-full bg-surface-2 px-2.5 py-1 text-[11.5px] text-muted">
          <Lock size={11} /> End-to-end encrypted
        </span>
      </header>

      <div
        ref={scroller}
        onScroll={(event) => {
          const element = event.currentTarget;
          isPinnedToBottom.current = element.scrollHeight - element.scrollTop - element.clientHeight < NEAR_EDGE_PX;
          if (element.scrollTop < NEAR_EDGE_PX) loadOlder();
        }}
        className="flex-1 overflow-y-auto py-3"
      >
        {hasOlder && (
          <div className="flex justify-center py-2 text-faint">
            <Spinner size={14} />
          </div>
        )}
        {messages && messages.length === 0 && (
          <EmptyState icon={<Hash size={22} />} title={`This is the start of #${channel?.name ?? ""}`}>
            Say hi. Only workspace members can read what you write.
          </EmptyState>
        )}
        <AnimatePresence initial={false}>
          {rows.map((row) =>
            row.kind === "day" ? (
              <DayDivider key={row.key} label={row.label} />
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
                  repliedTo={row.message.reply_to ? byId.get(row.message.reply_to) : undefined}
                  isHighlighted={highlighted === row.message.id}
                  onReply={() => setReplyTo(row.message)}
                  onJumpTo={jumpTo}
                />
              </motion.div>
            ),
          )}
        </AnimatePresence>
      </div>

      <Composer
        channelId={channelId}
        channelName={channel?.name ?? ""}
        replyTo={replyTo}
        onCancelReply={() => setReplyTo(null)}
        onSent={() => {
          isPinnedToBottom.current = true;
          setReplyTo(null);
        }}
      />
    </div>
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
