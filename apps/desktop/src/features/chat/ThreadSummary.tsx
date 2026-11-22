import { ChevronRight } from "lucide-react";
import type { Message } from "../../lib/api";
import { plainPreview } from "../../lib/markdown";
import { useMembers } from "../../lib/members";
import { displayName, formatWhen } from "../../lib/people";
import { CountBadge, cx, RelativeTime } from "../../ui";
import { ReplierAvatars } from "./ReplierAvatars";

/**
 * One line under a thread's first message: the latest repliers, the reply
 * count, and who said what last. The whole line is one button that opens
 * the thread.
 */
export function ThreadSummary({
  message,
  pageTitle,
  onOpen,
}: {
  message: Message;
  pageTitle: (id: string) => string | undefined;
  onOpen: () => void;
}) {
  const members = useMembers();
  const memberFor = (id: string) => members?.find((member) => member.id === id);
  const nameOf = (id: string) => displayName(memberFor(id)?.name ?? "", id);

  const count = message.reply_count;
  const countText = `${count} ${count === 1 ? "reply" : "replies"}`;
  const unread = message.unread_replies;
  const lastAuthor = message.last_reply_author === null ? null : nameOf(message.last_reply_author);
  const preview = message.last_reply_preview === null ? "" : plainPreview(message.last_reply_preview, pageTitle);
  const label = [
    countText,
    lastAuthor !== null && message.last_reply_ms !== null
      ? `last from ${lastAuthor} at ${formatWhen(message.last_reply_ms, new Date())}`
      : null,
    unread > 0 ? `${unread} new` : null,
  ]
    .filter((part) => part !== null)
    .join(", ");

  return (
    <button
      type="button"
      onClick={onOpen}
      aria-label={`${label}. Open thread`}
      className="group/thread mt-1 -ml-1.5 flex w-fit max-w-full min-w-0 items-center gap-2 rounded-md px-1.5 py-0.5 text-sm transition-colors hover:bg-surface-2"
    >
      <ReplierAvatars
        authorIds={message.recent_reply_authors}
        ringClassName="ring-surface group-hover/thread:ring-surface-2"
      />
      <span className={cx("shrink-0 whitespace-nowrap text-accent-text", unread > 0 ? "font-semibold" : "font-medium")}>
        {countText}
      </span>
      {unread > 0 && <CountBadge>{unread} new</CountBadge>}
      {lastAuthor !== null && preview !== "" && (
        <span className="min-w-0 truncate text-faint group-hover/thread:text-muted">
          {lastAuthor}: {preview}
        </span>
      )}
      {message.last_reply_ms !== null && (
        <RelativeTime
          ms={message.last_reply_ms}
          className="shrink-0 whitespace-nowrap text-faint group-hover/thread:text-muted"
        />
      )}
      <ChevronRight
        size={14}
        className="-ml-1 shrink-0 text-decorative opacity-0 transition-opacity group-hover/thread:opacity-100 group-focus-visible/thread:opacity-100"
      />
    </button>
  );
}
