import { useMemo } from "react";
import { api, type Channel, type Member } from "./api";
import { displayName } from "./people";
import { useLive } from "./useLive";
import { useWorkspace } from "./workspace";

/** A channel's name as the app shows it: its own, or for a DM, the other people in it. */
export function channelTitle(channel: Channel, members: readonly Member[] | undefined): string {
  if (channel.kind !== "dm") return channel.name;
  return channel.dm_members
    .map((id) => displayName(members?.find((member) => member.id === id)?.name ?? "", id))
    .join(", ");
}

/** How text refers to a channel: "#plans", or for a DM, "Ada". */
export function channelReference(channel: Channel | undefined, members: readonly Member[] | undefined): string {
  if (!channel) return "";
  return channel.kind === "dm" ? channelTitle(channel, members) : `#${channel.name}`;
}

/** What the composer asks for: "Message #plans" or "Message Ada". */
export function composerLabel(channel: Channel | undefined, members: readonly Member[] | undefined): string {
  return `Message ${channelReference(channel, members)}`;
}

/** Names channels by id, as `channelReference` does, for screens that list several. Stable while nothing changes. */
export function useChannelNames(): {
  reference: (channelId: string) => string;
  title: (channelId: string) => string;
} {
  const { channels } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members");
  return useMemo(() => {
    const find = (channelId: string) => channels.find((channel) => channel.id === channelId);
    return {
      reference: (channelId: string) => channelReference(find(channelId), members),
      title: (channelId: string) => {
        const channel = find(channelId);
        return channel ? channelTitle(channel, members) : "";
      },
    };
  }, [channels, members]);
}
