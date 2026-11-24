import { Lock, Quote } from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useMemo } from "react";
import { api, type LinkedMessage } from "../../lib/api";
import { channelReference } from "../../lib/channels";
import { useMemberNames, useMembers } from "../../lib/members";
import { displayName } from "../../lib/people";
import { plainPreview } from "../../lib/plainPreview";
import { useLive } from "../../lib/useLive";
import { linkedMessageView, useWorkspace } from "../../lib/workspace";
import { tweens } from "../../ui";
import { ChannelName } from "../private/ChannelName";

/** A message shared by link: who said it and where; opens it in place. */
export function MessageCard({ messageId }: { messageId: string }) {
  const { channels, pages, navigate, session } = useWorkspace();
  const members = useMembers();
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const memberName = useMemberNames();
  const loadLinked = useCallback(() => api.linkedMessage(messageId), [messageId]);
  // One that's missing could arrive in any channel; one that's shown changes only with its own.
  const linked: LinkedMessage | null | undefined = useLive(
    loadLinked,
    (change) => change.kind === "messages" && (!linked || change.channel === linked.message.channel),
  );
  if (linked === undefined) return null;
  if (linked === null) {
    return (
      <div
        role="note"
        className="mt-1.5 flex w-full max-w-card items-start gap-3 rounded-xl border border-line border-dashed p-3"
      >
        <Lock size={15} className="mt-0.5 shrink-0 text-decorative" aria-hidden />
        <span className="min-w-0 flex-1">
          <span className="block text-sm text-muted">A message you can't see</span>
          <span className="mt-0.5 block text-xs text-faint">
            It may be in a private channel you're not in, or deleted.
          </span>
        </span>
      </div>
    );
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
      whileHover={{ y: -2 }}
      transition={tweens.fast}
      aria-label={`Message from ${author} in ${where}`}
      onClick={() => navigate(linkedMessageView(linked))}
      className="mt-1.5 flex w-full max-w-card items-start gap-3 rounded-xl border border-line bg-surface p-3 text-left shadow-card transition-colors hover:border-accent/40"
    >
      <Quote size={15} className="mt-0.5 shrink-0 text-decorative" />
      <span className="min-w-0 flex-1">
        <span className="block text-xs text-muted">
          <b className="font-semibold text-ink">{author}</b> in <ChannelName channelId={message.channel} />
        </span>
        <span className="mt-0.5 line-clamp-2 block text-sm">
          {plainPreview(message.text, (id) => titles.get(id), memberName, session.workspace_id)}
        </span>
      </span>
    </motion.button>
  );
}
