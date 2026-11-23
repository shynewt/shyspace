import { Headphones } from "lucide-react";
import { motion } from "motion/react";
import type { ReactNode } from "react";
import type { Channel } from "../../lib/api";
import { shortcutKeys } from "../../lib/shortcuts";
import { AvatarStack, cx, Kbd, Spinner, springs, Tooltip } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";
import { useMyHuddle } from "./HuddleContext";
import { namesList } from "./huddleText";
import { useHuddle, useHuddleNames, useMemberNames } from "./useHuddle";

const PILL =
  "flex h-7 shrink-0 items-center gap-1.5 rounded-full px-2.5 text-sm whitespace-nowrap transition-colors @max-header-labels/header:px-1.5";

/**
 * The channel header's one huddle button: start one, join who's in, see it's
 * full, or leave. One click, no lobby. The faces show who's in, never how
 * their audio is doing; that's for the people in the huddle.
 */
export function HuddleButton({ channel }: { channel: Channel }) {
  const huddle = useHuddle(channel.id);
  const { mine, isInHere, join, leave } = useMyHuddle();
  const nameOf = useHuddleNames();
  const avatarName = useMemberNames();
  if (!huddle) return null;

  const isHere = isInHere(channel.id);
  const participants = huddle.participants;
  const others = participants
    .filter((participant) => !participant.is_me)
    .map((participant) => nameOf(participant.member));
  const faces = (
    <AvatarStack
      avatars={participants.map((participant) => ({
        key: participant.member,
        avatar: <MemberAvatar id={participant.member} name={avatarName(participant.member)} size={18} />,
      }))}
    />
  );

  if (isHere && mine?.phase === "joining") {
    return (
      <Pill tooltip="Joining…" className="cursor-default bg-surface-2 text-muted">
        <Spinner size={13} />
        <Label>Joining…</Label>
      </Pill>
    );
  }
  if (isHere) {
    return (
      <Pill tooltip="Leave the huddle" onClick={leave} className="bg-surface-2 pl-1.5 text-ink hover:bg-surface-3">
        {faces}
        <Label>Leave</Label>
      </Pill>
    );
  }
  if (huddle.is_full) {
    return (
      <Tooltip label={`This huddle is full: ${namesList(others)}`} side="bottom">
        <button
          type="button"
          aria-disabled
          aria-label={`Huddle full: ${namesList(others)}`}
          className={cx(PILL, "cursor-default bg-surface-2 pl-1.5 text-muted")}
        >
          {faces}
          <Label>Full</Label>
        </button>
      </Tooltip>
    );
  }
  if (participants.length > 0) {
    return (
      <Pill
        tooltip={`Join ${namesList(others)}`}
        onClick={() => join(channel.id)}
        className="bg-accent-soft pl-1.5 text-accent-text hover:bg-accent-soft/70"
      >
        {faces}
        <Label>Join</Label>
      </Pill>
    );
  }
  return (
    <Pill
      tooltip="Start a huddle"
      onClick={() => join(channel.id)}
      className="text-muted hover:bg-surface-2 hover:text-ink"
    >
      <Headphones size={15} />
      <Label>Huddle</Label>
    </Pill>
  );
}

/** Folds away in a narrow header, like `HeaderAction`'s label; the tooltip and name still say it. */
function Label({ children }: { children: string }) {
  return <span className="@max-header-labels/header:hidden">{children}</span>;
}

/**
 * Every state renders this one button, so keyboard focus stays on it as the
 * huddle changes. Without `onClick` it has no action for now, and says so
 * while staying focusable.
 */
function Pill({
  tooltip,
  onClick,
  className,
  children,
}: {
  tooltip: string;
  onClick?: () => void;
  className: string;
  children: ReactNode;
}) {
  return (
    <Tooltip
      label={
        <span className="flex items-center gap-2">
          {tooltip}
          <Kbd>{shortcutKeys("huddle").join(" ")}</Kbd>
        </span>
      }
      side="bottom"
    >
      <motion.button
        type="button"
        aria-label={tooltip}
        aria-disabled={onClick ? undefined : true}
        whileTap={onClick ? { scale: 0.95 } : undefined}
        transition={springs.snappy}
        onClick={onClick}
        className={cx(PILL, className)}
      >
        {children}
      </motion.button>
    </Tooltip>
  );
}
