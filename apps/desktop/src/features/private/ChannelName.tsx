import { channelTitle } from "../../lib/channels";
import { useMembers } from "../../lib/members";
import { useWorkspace } from "../../lib/workspace";
import { cx } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";
import { ChannelGlyph } from "./ChannelGlyph";

/**
 * A channel named inline in text: # and its name, a lock and the name for a
 * private one, or a DM's person (their avatar and name). Nothing for a
 * channel this device doesn't list. Accessible labels say it with
 * `channelReference` instead.
 */
export function ChannelName({ channelId, className }: { channelId: string; className?: string }) {
  const { channels } = useWorkspace();
  const members = useMembers();
  const channel = channels.find((candidate) => candidate.id === channelId);
  if (!channel) return null;
  const title = channelTitle(channel, members);
  const [person] = channel.dm_members;
  return (
    <span className={cx("inline-flex max-w-full min-w-0 items-center gap-1 align-bottom", className)}>
      {channel.kind === "dm" ? (
        person && <MemberAvatar id={person} name={title} size={14} />
      ) : (
        <ChannelGlyph kind={channel.kind} size={12} className="shrink-0 text-decorative" />
      )}
      <span className="truncate">{title}</span>
    </span>
  );
}
