import { Hash, Lock, NotebookPen, Users } from "lucide-react";
import { useState } from "react";
import type { Channel, Member } from "../../lib/api";
import { channelTitle } from "../../lib/channels";
import { IconButton } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";
import { PrivateMembersDialog } from "./PrivateMembersDialog";

/** A channel's heading: #name, a lock and the people button for a private one, a DM's people, or your notes. */
export function ChannelTitle({ channel, members }: { channel: Channel; members: Member[] | undefined }) {
  const title = channelTitle(channel, members);
  return (
    <>
      <TitleMark channel={channel} title={title} />
      <h1 className="truncate text-lg font-semibold">{title}</h1>
      {channel.kind === "notes" && (
        <span className="shrink-0 text-xs text-faint @max-header-labels/header:hidden">Only you can see them</span>
      )}
      {channel.kind === "private" && <PrivatePeopleButton channel={channel} />}
    </>
  );
}

function TitleMark({ channel, title }: { channel: Channel; title: string }) {
  switch (channel.kind) {
    case "dm": {
      const [first] = channel.dm_members;
      return first ? <MemberAvatar id={first} name={title} size={22} /> : null;
    }
    case "notes":
      return <NotebookPen size={16} className="text-decorative" />;
    case "public":
      return <Hash size={17} className="text-decorative" />;
    case "private":
      return <Lock size={16} className="text-decorative" aria-label="Private channel" />;
  }
}

function PrivatePeopleButton({ channel }: { channel: Channel }) {
  const [isShowingMembers, setShowingMembers] = useState(false);
  return (
    <>
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
