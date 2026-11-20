import { motion } from "motion/react";
import type { SyncStatus } from "../../lib/api";
import { cx, Tooltip } from "../../ui";

type Tone = "live" | "idle" | "error" | "pending";

const DOT: Record<Tone, string> = {
  live: "bg-success",
  idle: "bg-faint",
  error: "bg-danger",
  pending: "bg-decorative",
};

function describeStatus(status: SyncStatus | undefined): { tone: Tone; label: string; detail: string } {
  if (!status) return { tone: "pending", label: "Connecting…", detail: "Starting up." };
  if (status.error) return { tone: "error", label: "Sync problem", detail: status.error };
  if (status.needs_update) {
    return {
      tone: "error",
      label: "Update shyspace",
      detail: "A teammate uses a newer version. Some of their changes wait until you update.",
    };
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
  const { tone, label, detail } = describeStatus(status);
  return (
    <Tooltip label={<span className="block max-w-[260px] whitespace-normal">{detail}</span>} side="bottom">
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
