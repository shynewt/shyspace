import { motion } from "motion/react";
import type { SyncStatus } from "../lib/api";

export function SyncIndicator({ status }: { status: SyncStatus | undefined }) {
  const { tone, label, title } = describe(status);
  return (
    <div className="flex items-center gap-1.5 text-[11.5px] text-muted" title={title}>
      <span className="relative grid h-2 w-2 place-items-center">
        {tone === "live" && (
          <motion.span
            className="absolute inset-0 rounded-full bg-success"
            animate={{ scale: [1, 2.2], opacity: [0.5, 0] }}
            transition={{ duration: 1.8, repeat: Infinity, ease: "easeOut" }}
          />
        )}
        <span
          className={`relative h-2 w-2 rounded-full ${
            tone === "live" ? "bg-success" : tone === "error" ? "bg-danger" : tone === "idle" ? "bg-warning" : "bg-faint"
          }`}
        />
      </span>
      <span className="truncate">{label}</span>
    </div>
  );
}

function describe(status: SyncStatus | undefined) {
  if (!status) return { tone: "pending", label: "Connecting…", title: undefined };
  if (status.error) return { tone: "error", label: "Sync problem", title: status.error };
  if (status.peers > 0) {
    const label = `Live · ${status.peers} ${status.peers === 1 ? "peer" : "peers"}`;
    return { tone: "live", label, title: "Connected directly to teammates; the bucket keeps everything in sync." };
  }
  if (status.last_sync_ms) return { tone: "idle", label: "Synced via bucket", title: "No teammates online right now." };
  return { tone: "pending", label: "Connecting…", title: undefined };
}
