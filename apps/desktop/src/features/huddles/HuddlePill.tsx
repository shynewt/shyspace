import { Headphones, Mic, MicOff, PhoneOff } from "lucide-react";
import { motion } from "motion/react";
import type { MyHuddle } from "../../lib/api";
import { shortcutKeys } from "../../lib/shortcuts";
import { useWorkspace } from "../../lib/workspace";
import { IconButton, tweens } from "../../ui";
import { ChannelName } from "../private/ChannelName";
import { useMyHuddle } from "./HuddleContext";
import { useGoToHuddle } from "./HuddleElsewhere";

/** Your huddle in the header while the sidebar, and so its bar, is hidden: where, mute and leave. */
export function HuddlePill({ huddle: mine }: { huddle: MyHuddle }) {
  const { toggleMute, leave, isElsewhere } = useMyHuddle();
  const { navigate } = useWorkspace();
  const goThere = useGoToHuddle(mine);
  return (
    <motion.div
      role="group"
      aria-label="Your huddle"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, transition: tweens.exit }}
      className="flex h-7 shrink-0 items-center gap-0.5 rounded-full border border-line bg-surface pr-0.5 pl-2"
    >
      <Headphones size={13} className="shrink-0 text-accent" />
      <button
        type="button"
        onClick={isElsewhere ? goThere : () => navigate({ kind: "channel", id: mine.channel })}
        className="min-w-0 max-w-40 truncate px-1 text-sm hover:underline"
      >
        {isElsewhere ? mine.channel_name : <ChannelName channelId={mine.channel} />}
      </button>
      <IconButton
        label={mine.muted ? "Unmute" : "Mute"}
        shortcut={shortcutKeys("huddleMute")}
        aria-pressed={mine.muted}
        disabled={mine.mic === "permission_denied" || (mine.mic === "opening" && !mine.muted)}
        icon={mine.muted ? <MicOff size={14} className="text-danger-text" /> : <Mic size={14} />}
        size="sm"
        onClick={toggleMute}
      />
      <IconButton label="Leave the huddle" icon={<PhoneOff size={14} />} size="sm" onClick={leave} />
    </motion.div>
  );
}
