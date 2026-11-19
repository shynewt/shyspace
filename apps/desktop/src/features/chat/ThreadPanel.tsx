import { Bell, BellOff, X } from "lucide-react";
import { motion, useIsPresent } from "motion/react";
import { useCallback, useEffect } from "react";
import { api } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { Button, IconButton, Spinner, springs } from "../../ui";
import { Composer } from "./Composer";
import { MessageItem } from "./MessageItem";

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
  const replies = thread?.slice(1) ?? [];

  // Replies on screen while the window has focus count as read.
  const latestId = thread?.at(-1)?.id;
  const isPresent = useIsPresent();
  useEffect(() => {
    if (!latestId || !isPresent) return;
    const markRead = () => {
      if (document.hasFocus()) api.markThreadRead(root, latestId).catch(reportError);
    };
    markRead();
    window.addEventListener("focus", markRead);
    return () => window.removeEventListener("focus", markRead);
  }, [root, latestId, isPresent]);

  const isFocusShown = Boolean(focus && thread?.some((message) => message.id === focus));
  useEffect(() => {
    if (isFocusShown) document.getElementById(`message-${focus}`)?.scrollIntoView({ block: "center" });
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
        <h2 className="text-[14px] font-semibold">Thread</h2>
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
      <div className="flex-1 overflow-y-auto py-3">
        {!thread && (
          <div className="flex justify-center py-4 text-faint">
            <Spinner size={14} />
          </div>
        )}
        {first && <MessageItem message={first} isContinuation={false} isHighlighted={false} onOpenThread={null} />}
        {replies.length > 0 && (
          <div className="my-2 flex items-center gap-3 px-5">
            <span className="text-[11.5px] font-medium text-faint">
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
            isHighlighted={reply.id === focus}
            onOpenThread={null}
          />
        ))}
      </div>
      <Composer channelId={channelId} label="Reply in thread" threadRoot={root} />
    </motion.aside>
  );
}
