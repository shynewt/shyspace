import { useCallback, useEffect } from "react";
import { api } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";

/**
 * Shows outside the window how many things need you, counted as the sidebar
 * counts them: mentions and thread replies (the Inbox), messages in DMs you
 * haven't muted, and reminders that are due. A DM message is counted once.
 */
export function useAttentionBadge() {
  const { channels } = useWorkspace();
  const loadInbox = useCallback(() => api.inbox(), []);
  const inbox = useLive(loadInbox, (change) => change.kind === "messages" || change.kind === "channels");
  const later = useLive(api.later, (change) => change.kind === "later" || change.kind === "messages");
  const dms = new Set(channels.filter((channel) => channel.kind === "dm").map((channel) => channel.id));
  const outsideDms = inbox?.attention.filter((item) => !dms.has(item.message.channel)).length ?? 0;
  const inDms = channels
    .filter((channel) => channel.kind === "dm" && channel.notify !== "nothing")
    .reduce((total, channel) => total + channel.unread, 0);
  const now = Date.now();
  const due = later?.filter((item) => !item.is_done && item.remind_ms !== null && item.remind_ms <= now).length ?? 0;
  const count = outsideDms + inDms + due;
  useEffect(() => {
    api.setAttentionCount(count).catch(reportError);
  }, [count]);
}
