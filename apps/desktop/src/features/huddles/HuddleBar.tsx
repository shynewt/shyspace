import { ChevronDown, Headphones, Info, Mic, MicOff, PhoneOff, Volume2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useState } from "react";
import type { MicState, MyHuddle } from "../../lib/api";
import { shortcutKeys } from "../../lib/shortcuts";
import { useWorkspace } from "../../lib/workspace";
import { cx, IconButton, InlineNotice, Kbd, Menu, Popover, rise, Spinner, springs, Tooltip, tweens } from "../../ui";
import { ChannelName } from "../private/ChannelName";
import { HuddleAvatar } from "./HuddleAvatar";
import { useMyHuddle } from "./HuddleContext";
import { HuddleDetails } from "./HuddleDetails";
import { namesList } from "./huddleText";
import { useHuddle, useHuddleNames, useMemberNames } from "./useHuddle";
import { chooseDevice, deviceChoices, fullQualityMic, useHuddleDevices } from "./useHuddleDevices";

/**
 * Your huddle while you work, docked at the bottom of the sidebar on every
 * view: where it is, who's in and talking, mute, devices and leave, plus a
 * line for anything wrong with your microphone or headset.
 */
export function HuddleBar({ huddle }: { huddle: MyHuddle }) {
  const { session, navigate } = useWorkspace();
  const { speaking, reconnecting, isMicDown, isSpeakerDown, toggleMute, leave } = useMyHuddle();
  const roster = useHuddle(huddle.channel);
  const loaded = useHuddleDevices();
  const devices = loaded?.status === "shown" ? loaded.devices : undefined;
  const nameOf = useHuddleNames();
  const avatarName = useMemberNames();
  const [isDetailsOpen, setDetailsOpen] = useState(false);
  const needsUpdate = new Set(
    roster?.participants.filter((participant) => participant.needs_update).map((participant) => participant.member),
  );
  const everyone = [session.me, ...huddle.peers.map((peer) => peer.member)];
  const status = statusOf(huddle, reconnecting);
  const goodMic = devices && fullQualityMic(devices);

  return (
    <motion.section
      aria-label="Your huddle"
      {...rise}
      className="mx-2 mb-2 rounded-xl border border-line bg-surface p-2.5 shadow-card"
    >
      <div className="flex min-w-0 items-center gap-1.5">
        {huddle.phase === "joining" ? (
          <Spinner size={13} className="text-muted" />
        ) : (
          <Headphones size={14} className="shrink-0 text-accent" />
        )}
        <button
          type="button"
          onClick={() => navigate({ kind: "channel", id: huddle.channel })}
          className="min-w-0 text-sm font-medium hover:underline"
        >
          <ChannelName channelId={huddle.channel} />
        </button>
        {status && <span className="min-w-0 shrink-0 truncate text-xs text-faint">· {status}</span>}
        <div className="ml-auto">
          <Popover
            open={isDetailsOpen}
            onOpenChange={setDetailsOpen}
            side="right"
            align="end"
            trigger={<IconButton label="Huddle details" icon={<Info size={14} />} size="sm" />}
          >
            <HuddleDetails huddle={huddle} />
          </Popover>
        </div>
      </div>

      <button
        type="button"
        aria-label={`In the huddle: ${namesList(everyone.map(nameOf), everyone.length)}. Show details`}
        onClick={() => setDetailsOpen(true)}
        className="mt-2 flex w-full flex-wrap gap-1.5 rounded-md py-0.5"
      >
        <AnimatePresence initial={false}>
          {everyone.map((member) => {
            const peer = huddle.peers.find((candidate) => candidate.member === member);
            return (
              <motion.span
                key={member}
                initial={{ scale: 0.6, opacity: 0 }}
                animate={{ scale: 1, opacity: 1, transition: springs.pop }}
                exit={{ opacity: 0, transition: tweens.exit }}
                className="flex"
              >
                <HuddleAvatar
                  id={member}
                  name={avatarName(member)}
                  size={22}
                  isSpeaking={speaking.has(member)}
                  isConnecting={peer?.link === "connecting" || reconnecting.has(member)}
                  needsUpdate={needsUpdate.has(member)}
                />
              </motion.span>
            );
          })}
        </AnimatePresence>
      </button>

      <AnimatePresence initial={false}>
        {huddle.mic === "permission_denied" && (
          <Notice key="permission">
            <InlineNotice tone="danger">shyspace can't use your microphone. {PERMISSION_STEPS}</InlineNotice>
          </Notice>
        )}
        {huddle.mic === "silent" && (
          <Notice key="silent">
            <InlineNotice
              tone="warning"
              action={{ label: "Choose another microphone", onClick: () => setDetailsOpen(true) }}
            >
              Your microphone sends no sound. It may be muted on the device.
            </InlineNotice>
          </Notice>
        )}
        {isMicDown && (
          <Notice key="mic-down">
            <InlineNotice
              tone="warning"
              action={{ label: "Choose another microphone", onClick: () => setDetailsOpen(true) }}
            >
              Your microphone isn't working. Trying again…
            </InlineNotice>
          </Notice>
        )}
        {isSpeakerDown && (
          <Notice key="speaker-down">
            <InlineNotice
              tone="warning"
              action={{ label: "Choose other speakers", onClick: () => setDetailsOpen(true) }}
            >
              Your speakers aren't working. Trying again…
            </InlineNotice>
          </Notice>
        )}
        {huddle.output === "bluetooth_call_mode" && (
          <Notice key="call-quality">
            <InlineNotice
              tone="warning"
              action={
                devices && goodMic
                  ? { label: `Use ${goodMic.name}`, onClick: () => chooseDevice(devices, "input", goodMic.value) }
                  : undefined
              }
            >
              Your headset switched to call quality.
            </InlineNotice>
          </Notice>
        )}
      </AnimatePresence>

      <div className="mt-2 flex items-center">
        <MuteButton
          mic={huddle.mic}
          isMuted={huddle.muted}
          isJoining={huddle.phase === "joining"}
          onToggle={toggleMute}
        />
        {devices && (
          <Menu
            trigger={
              <button
                type="button"
                aria-label="Microphone and speakers"
                className="-ml-px grid h-7 w-6 place-items-center rounded-r-md border border-line bg-surface text-muted transition-colors hover:bg-surface-2 hover:text-ink"
              >
                <ChevronDown size={14} />
              </button>
            }
            items={[
              {
                label: "Microphone",
                icon: <Mic size={14} />,
                choices: deviceChoices(devices, "input").map((choice) => ({
                  label: choice.device?.is_bluetooth ? `${choice.label} (lowers sound quality)` : choice.label,
                  isChosen: choice.isChosen,
                  onSelect: () => chooseDevice(devices, "input", choice.value),
                })),
              },
              {
                label: "Speakers",
                icon: <Volume2 size={14} />,
                choices: deviceChoices(devices, "output").map((choice) => ({
                  label: choice.label,
                  isChosen: choice.isChosen,
                  onSelect: () => chooseDevice(devices, "output", choice.value),
                })),
              },
            ]}
          />
        )}
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

/** Where the OS keeps the microphone switch. */
const PERMISSION_STEPS = navigator.userAgent.includes("Mac")
  ? "Allow it in System Settings › Privacy & Security › Microphone."
  : navigator.userAgent.includes("Windows")
    ? "Allow it in Settings › Privacy & security › Microphone."
    : "Allow it in your system's privacy settings.";

/** What the top line adds after the place, if anything: trouble first, then mute, then the felt wait. */
function statusOf(huddle: MyHuddle, reconnecting: ReadonlySet<string>): string | null {
  if (huddle.phase === "joining") return null;
  const hasPeers = huddle.peers.length > 0;
  if (hasPeers && huddle.peers.every((peer) => reconnecting.has(peer.member))) return "Reconnecting…";
  if (huddle.muted) return "Muted";
  if (!hasPeers) return "Just you so far";
  if (huddle.peers.every((peer) => peer.link === "connecting")) return "Connecting…";
  return null;
}

function Notice({ children }: { children: ReactNode }) {
  return (
    <motion.div {...rise} className="mt-2">
      {children}
    </motion.div>
  );
}

/** Mute says its state in words, not only an icon: icons alone are why people talk on mute. */
function MuteButton({
  mic,
  isMuted,
  isJoining,
  onToggle,
}: {
  mic: MicState;
  isMuted: boolean;
  isJoining: boolean;
  onToggle: () => void;
}) {
  const isBlocked = mic === "permission_denied";
  const isOpening = mic === "opening" && !isMuted;
  const label = isOpening ? "Unmuting…" : isMuted ? "Unmute" : "Mute";
  return (
    <Tooltip
      label={
        <span className="flex items-center gap-2">
          {isMuted ? "Unmute" : "Mute"}
          <Kbd>{shortcutKeys("huddleMute").join(" ")}</Kbd>
        </span>
      }
    >
      <motion.button
        type="button"
        aria-pressed={isMuted}
        aria-label="Mute"
        disabled={isBlocked || isOpening || isJoining}
        whileTap={{ scale: 0.96 }}
        transition={springs.snappy}
        onClick={onToggle}
        className={cx(
          "flex h-7 items-center gap-1.5 rounded-l-md border px-2.5 text-sm transition-colors disabled:cursor-default disabled:opacity-60",
          isMuted
            ? "border-danger/30 bg-danger/8 text-danger-text hover:bg-danger/12"
            : "border-line bg-surface text-ink hover:bg-surface-2",
        )}
      >
        {isMuted || isBlocked ? <MicOff size={14} /> : <Mic size={14} className={cx(isOpening && "opacity-50")} />}
        <span aria-hidden>{label}</span>
      </motion.button>
    </Tooltip>
  );
}
