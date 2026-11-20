/** The rows of a channel's message list: messages, day dividers and the new-messages line. */
import { Hash, Lock } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type { Channel, Member, Message, MessageCursor } from "../../lib/api";
import { channelTitle } from "../../lib/channels";
import { formatDay } from "../../lib/people";
import { EmptyState, Spinner, springs } from "../../ui";
import { MessageItem } from "./MessageItem";
import { isAfter } from "./useReadPosition";

const GROUP_WINDOW_MS = 5 * 60 * 1000;
export const UNREAD_LINE_ID = "unread-line";

export type Row =
  | { kind: "day"; key: string; label: string }
  | { kind: "unread"; key: string }
  | { kind: "message"; key: string; message: Message; isContinuation: boolean };

/** The first message by someone else after `after`. */
export function firstUnreadId(messages: Message[], after: MessageCursor | null, me: string): string | null {
  const first = messages.find((message) => message.author !== me && (!after || isAfter(message, after)));
  return first?.id ?? null;
}

export function toRows(messages: Message[], unreadFrom: string | null): Row[] {
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

export function UnreadLine() {
  return (
    <motion.div
      id={UNREAD_LINE_ID}
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

export function DayDivider({ label }: { label: string }) {
  return (
    <div className="my-3 flex items-center gap-3 px-5">
      <div className="h-px flex-1 bg-line" />
      <span className="text-[11.5px] font-medium text-faint">{label}</span>
      <div className="h-px flex-1 bg-line" />
    </div>
  );
}

export function MessageRows({
  channel,
  members,
  messages,
  rows,
  hasOlder,
  isHighlighted,
  onOpenThread,
  onEdit,
  onMention,
}: {
  channel: Channel | undefined;
  members: Member[] | undefined;
  messages: Message[] | undefined;
  rows: Row[];
  hasOlder: boolean;
  isHighlighted: (message: Message) => boolean;
  onOpenThread: (message: Message) => void;
  onEdit: (message: Message) => void;
  onMention: (name: string, memberId: string) => void;
}) {
  return (
    <>
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
              initial={{ opacity: 0, y: 10, scale: 0.99 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              transition={springs.smooth}
            >
              <MessageItem
                message={row.message}
                isContinuation={row.isContinuation}
                isHighlighted={isHighlighted(row.message)}
                onOpenThread={() => onOpenThread(row.message)}
                onEdit={() => onEdit(row.message)}
                onMention={onMention}
              />
            </motion.div>
          ),
        )}
      </AnimatePresence>
    </>
  );
}
