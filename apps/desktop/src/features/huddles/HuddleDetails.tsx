import { AudioLines, Mic, Volume2 } from "lucide-react";
import type { ReactNode } from "react";
import type { AudioDevices, HuddlePeer, MyHuddle } from "../../lib/api";
import { channelReference } from "../../lib/channels";
import { useWorkspace } from "../../lib/workspace";
import { cx, Select } from "../../ui";
import { HuddleAvatar } from "./HuddleAvatar";
import { useMyHuddle } from "./HuddleContext";
import { useHuddle, useHuddleNames, useMemberNames } from "./useHuddle";
import { chooseDevice, deviceInUse, useHuddleDevices } from "./useHuddleDevices";

/** The core's cap on a huddle. */
export const HUDDLE_LIMIT = 8;

/** Everyone in your huddle and how each is connected, then your microphone and speakers. */
export function HuddleDetails({ huddle }: { huddle: MyHuddle }) {
  const { session, channels, members } = useWorkspace();
  const { speaking, reconnecting } = useMyHuddle();
  const roster = useHuddle(huddle.channel);
  const loaded = useHuddleDevices();
  const devices = loaded?.status === "shown" ? loaded.devices : undefined;
  const nameOf = useHuddleNames();
  const avatarName = useMemberNames();
  const needsUpdate = new Set(
    roster?.participants.filter((participant) => participant.needs_update).map((participant) => participant.member),
  );
  const place = channelReference(
    channels.find((channel) => channel.id === huddle.channel),
    members,
  );
  const noteOf = (peer: HuddlePeer): ReactNode => {
    if (needsUpdate.has(peer.member)) {
      return <span className="text-warning-text">needs an update to talk with you</span>;
    }
    if (peer.link === "connecting") return "connecting…";
    if (reconnecting.has(peer.member)) return "reconnecting…";
    if (peer.link === "relayed") return "relayed: may sound worse";
    return null;
  };
  const rows = [
    { member: session.me, note: huddle.muted ? "muted" : null, isConnecting: false },
    ...huddle.peers.map((peer) => ({
      member: peer.member,
      note: noteOf(peer),
      isConnecting: peer.link === "connecting" || reconnecting.has(peer.member),
    })),
  ];
  return (
    <div className="flex w-80 flex-col p-2">
      <div className="flex items-baseline justify-between gap-2 px-1 pb-2">
        <p className="truncate text-sm font-medium">Huddle in {place}</p>
        <p className="shrink-0 text-xs text-faint">
          {rows.length} of {HUDDLE_LIMIT}
        </p>
      </div>
      <ul aria-label="In the huddle" className="flex flex-col">
        {rows.map((row) => {
          const isSpeaking = speaking.has(row.member);
          return (
            <li key={row.member} className="flex h-9 items-center gap-2.5 rounded-lg px-1">
              <HuddleAvatar
                id={row.member}
                name={avatarName(row.member)}
                size={24}
                isSpeaking={isSpeaking}
                isConnecting={row.isConnecting}
                needsUpdate={needsUpdate.has(row.member)}
              />
              <span className="min-w-0 flex-1 truncate text-sm">
                {nameOf(row.member)}
                {row.note && <span className="ml-1.5 text-xs text-faint">{row.note}</span>}
              </span>
              {/* A fixed slot, so a row never shifts as people start and stop talking. */}
              <span className="w-4 shrink-0 text-success-text">
                {isSpeaking && (
                  <>
                    <AudioLines size={14} aria-hidden />
                    <span className="sr-only">speaking</span>
                  </>
                )}
              </span>
            </li>
          );
        })}
      </ul>
      {devices && <DevicePickers devices={devices} />}
    </div>
  );
}

/** A microphone and a speakers list, as in Settings › Huddles. */
function DevicePickers({ devices }: { devices: AudioDevices }) {
  return (
    <div className="mt-2 flex flex-col gap-2 border-t border-line px-1 pt-3">
      <DeviceRow icon={<Mic size={14} />}>
        <DeviceSelect devices={devices} kind="input" className="min-w-0 flex-1" />
      </DeviceRow>
      <BluetoothMicHint devices={devices} className="pl-6" />
      <DeviceRow icon={<Volume2 size={14} />}>
        <DeviceSelect devices={devices} kind="output" className="min-w-0 flex-1" />
      </DeviceRow>
    </div>
  );
}

/** The microphone (`input`) or speakers (`output`) in use, to pick another. */
export function DeviceSelect({
  devices,
  kind,
  className,
}: {
  devices: AudioDevices;
  kind: "input" | "output";
  className?: string;
}) {
  const list = kind === "input" ? devices.inputs : devices.outputs;
  return (
    <Select
      aria-label={kind === "input" ? "Microphone" : "Speakers"}
      value={deviceInUse(list, devices.chosen[kind])?.id ?? ""}
      options={list.map((device) => ({
        value: device.id,
        label: device.is_default ? `${device.name} (default)` : device.name,
      }))}
      onChange={(id) => {
        const device = list.find((candidate) => candidate.id === id);
        if (device) chooseDevice(devices, kind, device);
      }}
      className={className}
    />
  );
}

/** What a Bluetooth headset's microphone costs, shown while one is in use. */
export function BluetoothMicHint({ devices, className }: { devices: AudioDevices; className?: string }) {
  if (!deviceInUse(devices.inputs, devices.chosen.input)?.is_bluetooth) return null;
  return (
    <p className={cx("text-xs text-muted", className)}>A headset's microphone lowers what you hear to call quality.</p>
  );
}

function DeviceRow({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center gap-2">
      <span aria-hidden className="text-decorative">
        {icon}
      </span>
      {children}
    </div>
  );
}
