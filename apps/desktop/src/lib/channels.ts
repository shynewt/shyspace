import type { Channel, Member } from "./api";
import { displayName } from "./people";

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
