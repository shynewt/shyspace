import { Headphones, PhoneOff } from "lucide-react";
import { motion } from "motion/react";
import type { MyHuddle } from "../../lib/api";
import { rise, springs } from "../../ui";
import { useWorkspaces } from "../shell/Workspaces";
import { MuteButton } from "./HuddleBar";
import { useMyHuddle } from "./HuddleContext";
import { useOtherWorkspaceName } from "./IncomingHuddle";

/**
 * Your huddle while another workspace is shown: where it is, mute and leave.
 * Its people and devices belong to the workspace holding it, so they show
 * there; the place takes you back to it.
 */
export function HuddleElsewhereBar({ huddle }: { huddle: MyHuddle }) {
  const { toggleMute, leave } = useMyHuddle();
  const goThere = useGoToHuddle(huddle);
  const workspace = useOtherWorkspaceName(huddle.workspace_id);
  return (
    <motion.section
      aria-label="Your huddle"
      {...rise}
      className="mx-2 mb-2 rounded-xl border border-line bg-surface p-2.5 shadow-card"
    >
      <div className="flex min-w-0 items-center gap-1.5">
        <Headphones size={14} className="shrink-0 text-accent" />
        <button
          type="button"
          onClick={goThere}
          className="min-w-0 truncate text-left text-sm font-medium hover:underline"
        >
          {workspace} · {huddle.channel_name}
        </button>
        {huddle.muted && <span className="shrink-0 text-xs text-faint">· Muted</span>}
      </div>
      <div className="mt-2 flex items-center">
        <MuteButton
          mic={huddle.mic}
          isMuted={huddle.muted}
          isJoining={huddle.phase === "joining"}
          onToggle={toggleMute}
        />
        <motion.button
          type="button"
          whileTap={{ scale: 0.96 }}
          transition={springs.snappy}
          onClick={leave}
          className="ml-auto flex h-7 items-center gap-1.5 rounded-md px-2.5 text-sm text-danger-text transition-colors hover:bg-danger/10"
        >
          <PhoneOff size={14} />
          Leave
        </motion.button>
      </div>
    </motion.section>
  );
}

/** Shows the workspace holding `huddle`, at its channel. */
export function useGoToHuddle(huddle: MyHuddle): () => void {
  const { select } = useWorkspaces();
  return () => select(huddle.workspace_id, (navigate) => navigate({ kind: "channel", id: huddle.channel }));
}
