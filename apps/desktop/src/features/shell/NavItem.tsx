import { motion } from "motion/react";
import type { DragEvent, ReactNode } from "react";
import { sameView, useWorkspace, type View } from "../../lib/workspace";
import { CountBadge, cx, instantAfterAKey, springs } from "../../ui";
import { HuddleMarker, type HuddleSummary } from "../huddles/HuddleMarker";

export const INDENT_PX = 14;

export function NavItem({
  view,
  icon,
  label,
  kind,
  unread = 0,
  countNoun = "unread",
  mentions = 0,
  isForYou = false,
  waiting,
  isMuted = false,
  status,
  huddle = null,
  onContextMenu,
  onOpen,
  onDragStart,
  depth = 0,
  leading,
  trailing,
}: {
  view: View;
  icon: ReactNode;
  label: string;
  /** What it is, for screen readers, when the icon alone says it ("private channel"). */
  kind?: string;
  /** Messages you haven't read; shown as a count and bold label. */
  unread?: number;
  /** What the count is, for screen readers ("unread" unless said). */
  countNoun?: string;
  /** How many of those mention you; shown as a red badge instead. */
  mentions?: number;
  /** A DM: every message is for you, so unread ones count red, as a plain number rather than "@N". */
  isForYou?: boolean;
  /** Things only waiting, like drafts: a quiet count that never makes the label bold. `label` says it ("2 drafts"). */
  waiting?: { count: number; label: string };
  /** Muted: dimmed, and only mentions get a badge. */
  isMuted?: boolean;
  /** A small icon after the label, like a channel's alert level. */
  status?: { icon: ReactNode; label: string } | null;
  /** A huddle going on here: a quiet mark before any count. */
  huddle?: HuddleSummary | null;
  /** A right-click (or the keyboard's menu key) on the item. */
  onContextMenu?: () => void;
  /** What a click does instead of opening `view`, for a place made on first open. */
  onOpen?: () => void;
  /** Makes the item draggable, with what it carries. */
  onDragStart?: (event: DragEvent) => void;
  depth?: number;
  /** Rendered before the item, outside its button (like a fold toggle). */
  leading?: ReactNode;
  trailing?: ReactNode;
}) {
  const { view: current, navigate } = useWorkspace();
  const isActive = sameView(current, view);
  const forYou = isForYou ? (isMuted ? 0 : unread) : mentions;
  const hasUnread = !isActive && (isMuted ? forYou > 0 : unread > 0);
  const waitingLabel = waiting && waiting.count > 0 ? waiting.label : undefined;
  return (
    <div className="group relative flex items-center" style={{ paddingLeft: depth * INDENT_PX }}>
      {isActive && (
        <motion.div
          layoutId="nav-active"
          transition={instantAfterAKey(springs.snappy)}
          className="absolute inset-y-0 right-0 rounded-lg bg-surface shadow-card"
          style={{ left: depth * INDENT_PX }}
        />
      )}
      {/* Under the whole row, "…" included, like the selected highlight. */}
      {!isActive && (
        <div
          aria-hidden
          className="pointer-events-none absolute inset-y-0 right-0 rounded-lg bg-surface-2/70 opacity-0 group-hover:opacity-100"
          style={{ left: depth * INDENT_PX }}
        />
      )}
      {leading}
      <button
        type="button"
        aria-current={isActive ? "page" : undefined}
        aria-label={
          hasUnread || status || kind || waitingLabel || huddle
            ? accessibleName(
                [label, kind, status?.label, huddle?.label, waitingLabel],
                hasUnread ? unread : 0,
                isForYou ? 0 : mentions,
                countNoun,
              )
            : undefined
        }
        onClick={onOpen ?? (() => navigate(view))}
        draggable={onDragStart !== undefined}
        onDragStart={onDragStart}
        onContextMenu={
          onContextMenu &&
          ((event) => {
            event.preventDefault();
            onContextMenu();
          })
        }
        className={cx(
          "focus-ring-inset relative flex h-8 min-w-0 flex-1 items-center gap-2 rounded-lg px-2.5 text-base",
          isActive ? "font-medium text-ink" : "text-muted group-hover:text-ink",
          hasUnread && "font-semibold text-ink",
          isMuted && !hasUnread && !isActive && "opacity-60",
        )}
      >
        <span className={isActive ? "text-accent" : "text-decorative"}>{icon}</span>
        <span className="truncate">{label}</span>
        {status && (
          <span aria-hidden className="shrink-0 text-faint">
            {status.icon}
          </span>
        )}
        {huddle && (
          <span className="ml-auto flex">
            <HuddleMarker summary={huddle} />
          </span>
        )}
        {hasUnread && (
          <CountBadge
            aria-hidden
            key={`${unread}:${mentions}`}
            initial={{ scale: 0.6, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={springs.pop}
            tone={forYou > 0 ? "danger" : "accent"}
            className={cx(huddle ? "ml-1" : "ml-auto", "group-hover:opacity-0")}
          >
            {mentions > 0 && !isForYou ? `@${mentions}` : unread}
          </CountBadge>
        )}
        {!hasUnread && waiting && waiting.count > 0 && (
          <CountBadge aria-hidden tone="quiet" className={huddle ? "ml-1" : "ml-auto"}>
            {waiting.count}
          </CountBadge>
        )}
      </button>
      {trailing}
    </div>
  );
}

/** "random, muted, 2 unread, 1 mentions you": what the badges and icons show. */
function accessibleName(names: (string | undefined)[], unread: number, mentions: number, countNoun: string): string {
  const parts = names.filter((name): name is string => Boolean(name));
  if (unread > 0) parts.push(`${unread} ${countNoun}`);
  if (unread > 0 && mentions > 0) parts.push(`${mentions} ${mentions === 1 ? "mentions" : "mention"} you`);
  return parts.join(", ");
}
