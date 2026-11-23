import { Clock, Hash, Lock, NotebookPen, Users } from "lucide-react";
import { useState } from "react";
import type { Channel, Member } from "../../lib/api";
import { channelTitle } from "../../lib/channels";
import { deviceTimeZone, formatTimeIn, isFarAway, isKnownTimeZone } from "../../lib/localTime";
import { useMinuteClock } from "../../lib/useMinuteClock";
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
      {channel.kind === "dm" && channel.dm_members.length === 1 && (
        <TheirTime person={members?.find((member) => member.id === channel.dm_members[0])} />
      )}
    </>
  );
}

/** "10:00 AM their time", for someone hours away; nothing when they're close to your clock. */
function TheirTime({ person }: { person: Member | undefined }) {
  const now = useMinuteClock();
  const zone = person?.details.time_zone ?? "";
  if (!person || person.is_bot || !isKnownTimeZone(zone) || !isFarAway(zone, deviceTimeZone(), now)) return null;
  return (
    <span className="flex shrink-0 items-center gap-1 text-2xs text-faint @max-header-labels/header:hidden">
      <Clock size={11} aria-hidden className="text-decorative" />
      {formatTimeIn(zone, now)} their time
    </span>
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
