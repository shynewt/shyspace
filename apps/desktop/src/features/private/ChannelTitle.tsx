import { Hash, Lock, NotebookPen, Users } from "lucide-react";
import { useState } from "react";
import type { Channel, Member } from "../../lib/api";
import { channelTitle } from "../../lib/channels";
import { Avatar, IconButton } from "../../ui";
import { PrivateMembersDialog } from "./PrivateMembersDialog";

/** A channel's heading: #name, a lock and the people button for a private one, a DM's people, or your notes. */
export function ChannelTitle({ channel, members }: { channel: Channel; members: Member[] | undefined }) {
  const [isShowingMembers, setShowingMembers] = useState(false);
  const title = channelTitle(channel, members);
  if (channel.kind === "dm") {
    const [first] = channel.dm_members;
    return (
      <>
        {first && <Avatar id={first} name={title} size={22} />}
        <h1 className="truncate text-lg font-semibold">{title}</h1>
      </>
    );
  }
  if (channel.kind === "notes") {
    return (
      <>
        <NotebookPen size={16} className="text-decorative" />
        <h1 className="truncate text-lg font-semibold">{title}</h1>
        <span className="shrink-0 text-xs text-faint">Only you can see them</span>
      </>
    );
  }
  if (channel.kind === "public") {
    return (
      <>
        <Hash size={17} className="text-decorative" />
        <h1 className="truncate text-lg font-semibold">{title}</h1>
      </>
    );
  }
  return (
    <>
      <Lock size={16} className="text-decorative" aria-label="Private channel" />
      <h1 className="truncate text-lg font-semibold">{title}</h1>
      <IconButton
        label={`People in #${channel.name}`}
        size="sm"
        icon={<Users size={14} />}
        onClick={() => setShowingMembers(true)}
      />
      <PrivateMembersDialog channel={channel} open={isShowingMembers} onOpenChange={setShowingMembers} />
    </>
  );
}
