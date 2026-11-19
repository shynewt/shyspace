import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Hash, Lock } from "lucide-react";
import { api, type Message } from "../lib/api";
import { useLive } from "../lib/useLive";
import { formatDay } from "../lib/people";
import { useWorkspace } from "../lib/workspace";
import { MessageItem } from "./MessageItem";
import { Composer } from "./Composer";

const GROUP_WINDOW_MS = 5 * 60 * 1000;

type Row = { kind: "day"; key: string; label: string } | { kind: "message"; key: string; message: Message; isContinuation: boolean };

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
  const messages = useLive(
    () => api.messages(channelId),
    [channelId],
    (change) => (change.kind === "messages" && change.channel === channelId) || change.kind === "members",
  );
  const [replyTo, setReplyTo] = useState<Message | null>(null);
  const rows = useMemo(() => toRows(messages ?? []), [messages]);
  const byId = useMemo(() => new Map((messages ?? []).map((message) => [message.id, message])), [messages]);
  const scroller = useRef<HTMLDivElement>(null);
  const isPinnedToBottom = useRef(true);
  const [highlighted, setHighlighted] = useState<string | null>(null);

  useLayoutEffect(() => {
    const element = scroller.current;
    if (element && isPinnedToBottom.current) element.scrollTop = element.scrollHeight;
  }, [rows]);

  useEffect(() => {
    if (!highlighted) return;
    const timer = setTimeout(() => setHighlighted(null), 1600);
    return () => clearTimeout(timer);
  }, [highlighted]);

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
          isPinnedToBottom.current = element.scrollHeight - element.scrollTop - element.clientHeight < 80;
        }}
        className="flex-1 overflow-y-auto py-3"
      >
        {messages && messages.length === 0 && <EmptyChannel name={channel?.name ?? ""} />}
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
                transition={{ type: "spring", stiffness: 460, damping: 34 }}
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

function EmptyChannel({ name }: { name: string }) {
  return (
    <motion.div className="grid h-full place-items-center px-6 text-center" initial={{ opacity: 0 }} animate={{ opacity: 1 }}>
      <div>
        <div className="mx-auto grid h-12 w-12 place-items-center rounded-2xl bg-accent-soft text-accent-text">
          <Hash size={22} />
        </div>
        <h2 className="mt-3 font-semibold">This is the start of #{name}</h2>
        <p className="mt-1 text-[13px] text-muted">Say hi. Only workspace members can read what you write.</p>
      </div>
    </motion.div>
  );
}
