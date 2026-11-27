import { useMemo } from "react";
import { api, type Channel, type Member } from "./api";
import { displayName } from "./people";
import { useLive } from "./useLive";
import { useWorkspace } from "./workspace";

/**
 * A channel's name as the app shows it: its own, for a DM the other people in it ("Direct
 * message" once they've all left), or "Notes".
 */
export function channelTitle(channel: Channel, members: readonly Member[] | undefined): string {
  if (channel.kind === "notes") return "Notes";
  if (channel.kind !== "dm") return channel.name;
  return (
    channel.dm_members
      .map((id) => displayName(members?.find((member) => member.id === id)?.name ?? "", id))
      .join(", ") || "Direct message"
  );
}

/** How text refers to a channel: "#plans", or for a DM, "Ada". */
export function channelReference(channel: Channel | undefined, members: readonly Member[] | undefined): string {
  if (!channel) return "";
  if (channel.kind === "notes") return "your notes";
  return channel.kind === "dm" ? channelTitle(channel, members) : `#${channel.name}`;
}

/**
 * A notification's title for a message by `author`: "Grace" in a DM with you, "Grace in your group
 * with Alan", or "Grace in #plans". `fallbackName` is the channel's name from the alert, for a
 * channel this device hasn't listed yet.
 */
export function alertTitle(
  author: { id: string; name: string },
  channel: Channel | undefined,
  members: readonly Member[] | undefined,
  fallbackName: string,
): string {
  const name = displayName(author.name, author.id);
  if (channel?.kind !== "dm") return `${name} in #${channel?.name ?? fallbackName}`;
  const others = channel.dm_members.filter((id) => id !== author.id);
  if (others.length === 0) return name;
  const otherNames = others.map((id) => displayName(members?.find((member) => member.id === id)?.name ?? "", id));
  return `${name} in your group with ${otherNames.join(", ")}`;
}

/** What the composer asks for: "Message #plans", "Message Ada" or "Write a note". */
export function composerLabel(channel: Channel | undefined, members: readonly Member[] | undefined): string {
  if (channel?.kind === "notes") return "Write a note";
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
