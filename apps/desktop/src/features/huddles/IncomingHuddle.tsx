import { Headphones, PhoneOff } from "lucide-react";
import { motion } from "motion/react";
import type { IncomingHuddle } from "../../lib/api";
import { displayName } from "../../lib/people";
import { useWorkspace } from "../../lib/workspace";
import { Button, IconButton, rise, tweens } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";
import { ChannelName } from "../private/ChannelName";
import { useMyHuddle } from "./HuddleContext";

/**
 * A DM huddle ringing for you, docked where your huddle's bar goes: who,
 * and Join or Decline. It stays only while the core rings, and nothing in
 * it moves after it arrives: the ring is the alarm, the card just answers it.
 */
export function IncomingHuddleCard({ incoming }: { incoming: IncomingHuddle }) {
  const { join, decline } = useMyHuddle();
  const { channels } = useWorkspace();
  const name = displayName(incoming.member_name, incoming.member);
  const isGroup = (channels.find((channel) => channel.id === incoming.channel)?.dm_members.length ?? 0) > 1;
  return (
    <motion.section
      aria-label="Incoming huddle"
      {...rise}
      className="mx-2 mb-2 rounded-xl border border-line bg-surface p-2.5 shadow-card"
    >
      <div className="flex min-w-0 items-center gap-2.5">
        <MemberAvatar id={incoming.member} name={name} size={28} />
        <div className="min-w-0">
          <p className="truncate text-sm">
            <span className="font-medium">{name}</span> started a huddle
          </p>
          {isGroup && (
            <p className="truncate text-xs text-muted">
              <ChannelName channelId={incoming.channel} />
            </p>
          )}
        </div>
      </div>
      <div className="mt-2.5 flex gap-1.5">
        <Button
          variant="primary"
          size="sm"
          icon={<Headphones size={14} />}
          onClick={() => join(incoming.channel)}
          className="flex-1 justify-center"
        >
          Join
        </Button>
        <Button size="sm" onClick={decline} className="flex-1 justify-center">
          Decline
        </Button>
      </div>
    </motion.section>
  );
}

/** The same, in the header while the sidebar is hidden. */
export function IncomingHuddlePill({ incoming }: { incoming: IncomingHuddle }) {
  const { join, decline } = useMyHuddle();
  const name = displayName(incoming.member_name, incoming.member);
  return (
    <motion.div
      role="group"
      aria-label="Incoming huddle"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, transition: tweens.exit }}
      className="flex h-7 shrink-0 items-center gap-1 rounded-full border border-line bg-surface pr-0.5 pl-1"
    >
      <MemberAvatar id={incoming.member} name={name} size={20} />
      <span className="max-w-32 truncate px-0.5 text-sm">{name}</span>
      <button
        type="button"
        aria-label={`Join ${name}'s huddle`}
        onClick={() => join(incoming.channel)}
        className="flex h-6 items-center gap-1 rounded-full bg-accent-soft px-2 text-sm text-accent-text transition-colors hover:bg-accent-soft/70"
      >
        <Headphones size={13} />
        Join
      </button>
      <IconButton label="Decline" icon={<PhoneOff size={14} />} size="sm" onClick={decline} />
    </motion.div>
  );
}
