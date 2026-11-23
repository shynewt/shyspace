import { Bell, BellOff, Eye, EyeOff, Settings, Smile, TriangleAlert, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, type Member } from "../../lib/api";
import { displayName, formatStatusEnd, presenceLabel } from "../../lib/people";
import { reportError, showToast } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Menu, type MenuItem } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";
import { StorageAttentionSign, useStorageNeedsAttention } from "../settings/StorageProtection";
import { useWorkspaces } from "./Workspaces";

/** You, at the foot of the sidebar: your status, appearing offline, and Settings. */
export function MeMenu() {
  const { session, openSettings, openStatus } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members");
  const isOffline = useLive(api.appearOffline, (change) => change.kind === "members");
  const storageNeedsAttention = useStorageNeedsAttention();
  const { workspaces } = useWorkspaces();
  const [pausedUntil, setPaused] = useNotificationsPause();
  const me = members?.find((member) => member.id === session.me);
  if (!me || isOffline === undefined) return <div className="h-11" />;

  const name = displayName(me.name, me.id);
  const items: MenuItem[] = [];
  if (storageNeedsAttention) {
    items.push({
      label: "Storage needs attention",
      icon: <TriangleAlert size={14} className="text-danger" />,
      onSelect: () => openSettings({ section: "storage" }),
    });
  }
  items.push({
    label: me.status ? "Edit status" : "Set status",
    icon: <Smile size={14} />,
    onSelect: openStatus,
  });
  if (me.status) {
    items.push({
      label: "Clear status",
      icon: <X size={14} />,
      onSelect: () => api.clearStatus().then(() => showToast("Status cleared."), reportError),
    });
  }
  // One pause for every workspace; with only one, the status's alerts setting already does this.
  if (workspaces.length > 1) {
    items.push(
      pausedUntil !== null
        ? {
            label: `Resume notifications (paused until ${formatStatusEnd(pausedUntil)})`,
            icon: <Bell size={14} />,
            onSelect: () => setPaused(null),
          }
        : {
            label: "Pause notifications everywhere",
            icon: <BellOff size={14} />,
            choices: [
              { label: "For 1 hour", isChosen: false, onSelect: () => setPaused(Date.now() + 60 * 60 * 1000) },
              { label: "Until tomorrow", isChosen: false, onSelect: () => setPaused(tomorrowMorning()) },
            ],
          },
    );
  }
  items.push(
    {
      label: isOffline ? "Stop appearing offline" : "Appear offline",
      icon: isOffline ? <Eye size={14} /> : <EyeOff size={14} />,
      onSelect: () => api.setAppearOffline(!isOffline).catch(reportError),
    },
    { label: "Settings", icon: <Settings size={14} />, onSelect: () => openSettings() },
  );

  return (
    <Menu
      items={items}
      trigger={
        <button
          type="button"
          className="focus-ring-inset flex h-11 w-full min-w-0 items-center gap-2.5 rounded-lg px-2 text-left transition-colors hover:bg-surface-2 data-[state=open]:bg-surface-2"
        >
          <MemberAvatar id={me.id} name={name} size={28} presence={isOffline ? undefined : me.presence} />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium">{name}</span>
            <span className="block truncate text-xs text-muted">{statusLine(me, isOffline)}</span>
          </span>
          {pausedUntil !== null && workspaces.length > 1 && (
            <BellOff size={14} role="img" aria-label="Notifications paused" className="shrink-0 text-muted" />
          )}
          <StorageAttentionSign />
        </button>
      }
    />
  );
}

/** Whether every workspace's notifications are held, and until when; a pause that has run out reads as none. */
function useNotificationsPause(): [number | null, (untilMs: number | null) => void] {
  const [until, setUntil] = useState<number | null>(null);
  useEffect(() => {
    api.notificationsPaused().then(setUntil, reportError);
  }, []);
  useEffect(() => {
    if (until === null) return;
    const timer = window.setTimeout(() => setUntil(null), Math.max(until - Date.now(), 0));
    return () => clearTimeout(timer);
  }, [until]);
  const set = useCallback((untilMs: number | null) => {
    api.setNotificationsPaused(untilMs).then(() => setUntil(untilMs), reportError);
  }, []);
  return [until, set];
}

/** 9:00 tomorrow, this device's time. */
function tomorrowMorning(): number {
  const morning = new Date();
  morning.setDate(morning.getDate() + 1);
  morning.setHours(9, 0, 0, 0);
  return morning.getTime();
}

function statusLine(me: Member, isOffline: boolean): string {
  if (me.status) {
    const words = [me.status.emoji, me.status.text].filter(Boolean).join(" ");
    if (words) return words;
    if (me.status.alerts === "mentions") return "Alerts: mentions only";
    if (me.status.alerts === "nothing") return "Alerts paused";
  }
  if (isOffline) return "Appearing offline";
  return presenceLabel(me.presence);
}
