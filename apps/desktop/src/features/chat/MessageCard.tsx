import { Quote } from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useMemo } from "react";
import { api } from "../../lib/api";
import { channelReference } from "../../lib/channels";
import { plainPreview } from "../../lib/markdown";
import { useMembers } from "../../lib/members";
import { displayName } from "../../lib/people";
import { useLive } from "../../lib/useLive";
import { linkedMessageView, useWorkspace } from "../../lib/workspace";
import { springs } from "../../ui";
import { ChannelName } from "../private/ChannelName";

/** A message shared by link: who said it and where; opens it in place. */
export function MessageCard({ messageId }: { messageId: string }) {
  const { channels, pages, navigate } = useWorkspace();
  const members = useMembers();
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const loadLinked = useCallback(() => api.linkedMessage(messageId), [messageId]);
  const linked = useLive(loadLinked, (change) => change.kind === "messages");
  if (linked === undefined) return null;
  if (linked === null) {
    return <div className="mt-1.5 text-[12.5px] text-faint italic">A linked message that no longer exists</div>;
  }
  const { message } = linked;
  const author = displayName(message.author_name, message.author);
  const where = channelReference(
    channels.find((channel) => channel.id === message.channel),
    members,
  );
  return (
    <motion.button
      type="button"
      layout
      whileHover={{ y: -2 }}
      transition={springs.snappy}
      aria-label={`Message from ${author} in ${where}`}
      onClick={() => navigate(linkedMessageView(linked))}
      className="mt-1.5 flex w-full max-w-[440px] items-start gap-3 rounded-xl border border-line bg-surface p-3 text-left shadow-card transition-colors hover:border-accent/40"
    >
      <Quote size={15} className="mt-0.5 shrink-0 text-decorative" />
      <span className="min-w-0 flex-1">
        <span className="block text-[12px] text-muted">
          <b className="font-semibold text-ink">{author}</b> in <ChannelName channelId={message.channel} />
        </span>
        <span className="mt-0.5 line-clamp-2 block text-[13px]">
          {plainPreview(message.text, (id) => titles.get(id))}
        </span>
      </span>
    </motion.button>
  );
}
