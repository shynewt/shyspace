import { motion } from "motion/react";
import type { Member, SyncStatus } from "../../lib/api";
import { useMembers } from "../../lib/members";
import { displayName } from "../../lib/people";
import { cx, Tooltip } from "../../ui";

type Tone = "live" | "idle" | "error" | "pending";

const DOT: Record<Tone, string> = {
  live: "bg-success",
  idle: "bg-faint",
  error: "bg-danger",
  pending: "bg-decorative",
};

/**
 * Whose app sent what this version can't read, without guessing why: a newer
 * version looks the same as junk from a modified or buggy app.
 */
function unreadableNotice(ids: string[], members: Member[] | undefined): string {
  const names = ids.flatMap((id) => {
    const member = members?.find((candidate) => candidate.id === id);
    return member ? [displayName(member.name, id)] : [];
  });
  const list = (items: string[]) => new Intl.ListFormat("en-GB", { type: "conjunction" }).format(items);
  if (names.length === 0) {
    return "A teammate's app sent something this version can't read. Updating shyspace usually fixes it; if it doesn't, ask an admin.";
  }
  const shown = names.length > 3 ? [...names.slice(0, 2), `${names.length - 2} others`] : names;
  const whose = list(shown.map((name) => (name.endsWith(" others") ? `${name}'` : `${name}'s`)));
  const ask = names.length === 1 ? names[0] : "them";
  return `${whose} ${names.length === 1 ? "app" : "apps"} sent something this version can't read. Updating shyspace usually fixes it; if it doesn't, ask ${ask} or an admin.`;
}

function describeStatus(
  status: SyncStatus | undefined,
  members: Member[] | undefined,
): { tone: Tone; label: string; detail: string } {
  if (!status) return { tone: "pending", label: "Connecting…", detail: "Starting up." };
  if (status.error) return { tone: "error", label: "Sync problem", detail: status.error };
  if (status.needs_update) {
    return { tone: "error", label: "Update shyspace", detail: unreadableNotice(status.needs_update_from, members) };
  }
  if (status.peers > 0) {
    return {
      tone: "live",
      label: `Live · ${status.peers} ${status.peers === 1 ? "peer" : "peers"}`,
      detail: "Connected directly to teammates; the bucket keeps everything in sync.",
    };
  }
  if (status.last_sync_ms)
    return { tone: "idle", label: "Synced via bucket", detail: "No teammates online right now." };
  return { tone: "pending", label: "Connecting…", detail: "Reaching the bucket and peers." };
}

export function SyncIndicator({ status }: { status: SyncStatus | undefined }) {
  const { tone, label, detail } = describeStatus(status, useMembers());
  return (
    <Tooltip label={detail} side="bottom">
      <div className="flex w-fit items-center gap-1.5 text-xs text-muted">
        <span className="relative grid h-2 w-2 place-items-center">
          {tone === "live" && (
            <motion.span
              className="absolute inset-0 rounded-full bg-success"
              animate={{ scale: [1, 2.2], opacity: [0.5, 0] }}
              transition={{ duration: 1.8, repeat: Number.POSITIVE_INFINITY, ease: "easeOut" }}
            />
          )}
          <span className={cx("relative h-2 w-2 rounded-full", DOT[tone])} />
        </span>
        <span className="truncate">{label}</span>
      </div>
    </Tooltip>
  );
}
