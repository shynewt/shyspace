import { Bell, BellOff, X } from "lucide-react";
import { motion, useIsPresent } from "motion/react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type Message } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWindowAttention } from "../../lib/useWindowAttention";
import { Button, IconButton, Spinner, springs } from "../../ui";
import { WriteGate } from "../private/WriteGate";
import { Composer } from "./Composer";
import { MessageItem } from "./MessageItem";
import { knownReadPosition, useReadPosition } from "./useReadPosition";
import type { InsertMention } from "./useSuggestions";

const NEAR_END_PX = 80;

/** A thread beside its channel: the first message, its replies, and a reply box. */
export function ThreadPanel({
  root,
  focus,
  channelId,
  onClose,
}: {
  root: string;
  /** A reply to scroll to and highlight, from a message link. */
  focus?: string;
  channelId: string;
  onClose: () => void;
}) {
  const loadThread = useCallback(() => api.thread(root), [root]);
  const thread = useLive(
    loadThread,
    (change) => (change.kind === "messages" && change.channel === channelId) || change.kind === "members",
  );
  const first = thread?.[0];
  const [editing, setEditing] = useState<Message | null>(null);
  const mentionTarget = useRef<InsertMention | null>(null);
  const mention = useCallback((name: string, memberId: string) => mentionTarget.current?.(name, memberId), []);
  const replies = thread?.slice(1) ?? [];
  const scroller = useRef<HTMLDivElement>(null);
  // Follows new replies while you're at the end; a reply to show from a link holds the view instead.
  const isAtEnd = useRef(!focus);

  // Replies are read like channel messages, into the thread's own read position.
  const isPresent = useIsPresent();
  const isLooking = useWindowAttention() && isPresent;
  const { markSeen } = useReadPosition({
    key: root,
    list: scroller,
    messages: thread,
    initial: knownReadPosition(root, null),
    isLooking,
    save: (messageId) => api.markThreadRead(root, messageId),
  });

  useLayoutEffect(() => {
    const element = scroller.current;
    if (element && thread && isAtEnd.current) element.scrollTop = element.scrollHeight;
    markSeen();
  }, [thread, markSeen]);

  const isFocusShown = Boolean(focus && thread?.some((message) => message.id === focus));
  useEffect(() => {
    if (!isFocusShown) return;
    document.getElementById(`message-${focus}`)?.scrollIntoView({ block: "center" });
    // A short thread doesn't scroll, so no scroll event would say it's at its end.
    const element = scroller.current;
    if (element) isAtEnd.current = element.scrollHeight - element.scrollTop - element.clientHeight < NEAR_END_PX;
  }, [isFocusShown, focus]);

  const toggleFollowing = () => {
    if (first) api.setFollowing(root, !first.is_following).catch(reportError);
  };

  return (
    <motion.aside
      aria-label="Thread"
      initial={{ opacity: 0, x: 24 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 24 }}
      transition={springs.smooth}
      className="flex w-[380px] shrink-0 flex-col border-l border-line bg-surface"
    >
      <header className="flex h-13 shrink-0 items-center gap-2 border-b border-line px-4">
        <h2 className="text-base font-semibold">Thread</h2>
        {first && (
          <Button
            variant="ghost"
            size="sm"
            aria-pressed={first.is_following}
            icon={first.is_following ? <Bell size={13} /> : <BellOff size={13} />}
            onClick={toggleFollowing}
            className="ml-auto"
          >
            {first.is_following ? "Following" : "Follow"}
          </Button>
        )}
        <IconButton label="Close thread" icon={<X size={15} />} onClick={onClose} className={first ? "" : "ml-auto"} />
      </header>
      <div
        ref={scroller}
        onScroll={(event) => {
          const element = event.currentTarget;
          isAtEnd.current = element.scrollHeight - element.scrollTop - element.clientHeight < NEAR_END_PX;
          markSeen();
        }}
        className="relative flex-1 overflow-y-auto py-3"
      >
        {!thread && (
          <div className="flex justify-center py-4 text-decorative">
            <Spinner size={14} />
          </div>
        )}
        {first && (
          <MessageItem
            message={first}
            isContinuation={false}
            isHighlighted={editing?.id === first.id}
            onOpenThread={null}
            onEdit={() => setEditing(first)}
            onMention={mention}
          />
        )}
        {replies.length > 0 && (
          <div className="my-2 flex items-center gap-3 px-5">
            <span className="text-xs font-medium text-faint">
              {replies.length} {replies.length === 1 ? "reply" : "replies"}
            </span>
            <div className="h-px flex-1 bg-line" />
          </div>
        )}
        {replies.map((reply) => (
          <MessageItem
            key={reply.id}
            message={reply}
            isContinuation={false}
            isHighlighted={reply.id === focus || editing?.id === reply.id}
            onOpenThread={null}
            onEdit={() => setEditing(reply)}
            onMention={mention}
          />
        ))}
      </div>
      <WriteGate channelId={channelId}>
        <Composer
          channelId={channelId}
          label="Reply in thread"
          threadRoot={root}
          onSent={() => {
            isAtEnd.current = true;
          }}
          editing={editing}
          onEditDone={() => setEditing(null)}
          onEditLast={() => setEditing(thread?.findLast((message) => message.can_edit) ?? null)}
          mentionTarget={mentionTarget}
        />
      </WriteGate>
    </motion.aside>
  );
}
