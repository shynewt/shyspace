import type { ReactNode } from "react";
import { api, type Channel, type Member } from "../../lib/api";
import { channelTitle } from "../../lib/channels";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";

/** Why you can't write in `channel` any more, or null if you can. */
function whyReadOnly(channel: Channel | undefined, members: Member[] | undefined): string | null {
  if (channel?.is_member === false) {
    return "You're no longer in this channel. You can still read what was said while you were in it.";
  }
  if (channel?.kind !== "dm" || !members) return null;
  const others = members.filter((member) => channel.dm_members.includes(member.id));
  if (others.length === 0 || others.some((member) => member.active)) return null;
  return `${channelTitle(channel, members)} left the workspace, so this conversation is read-only.`;
}

/** Shows `children` (a composer) only where you can still write. */
export function WriteGate({ channelId, children }: { channelId: string; children: ReactNode }) {
  const { channels } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members");
  const notice = whyReadOnly(
    channels.find((channel) => channel.id === channelId),
    members,
  );
  if (!notice) return children;
  return (
    <p className="mx-5 mb-4 rounded-xl border border-line bg-surface-2 px-4 py-3 text-[13px] text-muted">{notice}</p>
  );
}
