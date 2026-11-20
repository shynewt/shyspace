import { motion } from "motion/react";
import type { ReactNode } from "react";
import { sameView, useWorkspace, type View } from "../../lib/workspace";
import { cx, springs } from "../../ui";

export const INDENT_PX = 14;

export function NavItem({
  view,
  icon,
  label,
  kind,
  unread = 0,
  countNoun = "unread",
  mentions = 0,
  isMuted = false,
  status,
  onContextMenu,
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
  /** Muted: dimmed, and only mentions get a badge. */
  isMuted?: boolean;
  /** A small icon after the label, like a channel's alert level. */
  status?: { icon: ReactNode; label: string } | null;
  /** A right-click (or the keyboard's menu key) on the item. */
  onContextMenu?: () => void;
  depth?: number;
  /** Rendered before the item, outside its button (like a fold toggle). */
  leading?: ReactNode;
  trailing?: ReactNode;
}) {
  const { view: current, navigate } = useWorkspace();
  const isActive = sameView(current, view);
  const hasUnread = !isActive && (isMuted ? mentions > 0 : unread > 0);
  return (
    <div className="group relative flex items-center" style={{ paddingLeft: depth * INDENT_PX }}>
      {isActive && (
        <motion.div
          layoutId="nav-active"
          transition={springs.snappy}
          className="absolute inset-y-0 right-0 rounded-lg bg-surface shadow-card"
          style={{ left: depth * INDENT_PX }}
        />
      )}
      {leading}
      <button
        type="button"
        aria-current={isActive ? "page" : undefined}
        aria-label={
          hasUnread || status || kind
            ? accessibleName([label, kind, status?.label], hasUnread ? unread : 0, mentions, countNoun)
            : undefined
        }
        onClick={() => navigate(view)}
        onContextMenu={
          onContextMenu &&
          ((event) => {
            event.preventDefault();
            onContextMenu();
          })
        }
        className={cx(
          "relative flex h-8 min-w-0 flex-1 items-center gap-2 rounded-lg px-2.5 text-[13.5px] transition-colors",
          isActive ? "font-medium text-ink" : "text-muted hover:bg-surface-2/70 hover:text-ink",
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
        {hasUnread && (
          <motion.span
            aria-hidden
            key={`${unread}:${mentions}`}
            initial={{ scale: 0.6, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={springs.pop}
            className={cx(
              "ml-auto grid h-[18px] min-w-[18px] place-items-center rounded-full px-1.5 text-[10.5px] font-semibold text-white tabular-nums group-hover:opacity-0",
              mentions > 0 ? "bg-danger" : "bg-accent",
            )}
          >
            {mentions > 0 ? `@${mentions}` : unread}
          </motion.span>
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
