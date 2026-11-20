import { Eye, EyeOff, Settings, Smile, X } from "lucide-react";
import { useState } from "react";
import { api, type Member } from "../../lib/api";
import { displayName } from "../../lib/people";
import { reportError, showToast } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, Menu, type MenuItem } from "../../ui";
import { StorageAttentionDot } from "../settings/StorageProtection";
import { StatusDialog } from "./StatusDialog";

const PRESENCE_LABELS: Record<Member["presence"], string> = {
  online: "Online",
  away: "Away",
  unreachable: "Not reachable right now",
};

/** You, at the foot of the sidebar: your status, appearing offline, and Settings. */
export function MeMenu() {
  const { session, openSettings } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members");
  const isOffline = useLive(api.appearOffline, (change) => change.kind === "members");
  const [isStatusOpen, setStatusOpen] = useState(false);
  const me = members?.find((member) => member.id === session.me);
  if (!me || isOffline === undefined) return <div className="h-11" />;

  const name = displayName(me.name, me.id);
  const items: MenuItem[] = [
    { label: me.status ? "Edit status" : "Set status", icon: <Smile size={14} />, onSelect: () => setStatusOpen(true) },
  ];
  if (me.status) {
    items.push({
      label: "Clear status",
      icon: <X size={14} />,
      onSelect: () => api.clearStatus().then(() => showToast("Status cleared."), reportError),
    });
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
    <>
      <Menu
        items={items}
        trigger={
          <button
            type="button"
            className="flex h-11 w-full min-w-0 items-center gap-2.5 rounded-lg px-2 text-left transition-colors hover:bg-surface-2 data-[state=open]:bg-surface-2"
          >
            <Avatar id={me.id} name={name} size={28} presence={isOffline ? undefined : me.presence} />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13px] font-medium">{name}</span>
              <span className="block truncate text-[11.5px] text-muted">{statusLine(me, isOffline)}</span>
            </span>
            <StorageAttentionDot />
          </button>
        }
      />
      <StatusDialog open={isStatusOpen} onOpenChange={setStatusOpen} current={me.status} />
    </>
  );
}

function statusLine(me: Member, isOffline: boolean): string {
  if (me.status) {
    const words = [me.status.emoji, me.status.text].filter(Boolean).join(" ");
    if (words) return words;
    if (me.status.alerts === "mentions") return "Alerts: mentions only";
    if (me.status.alerts === "nothing") return "Alerts paused";
  }
  if (isOffline) return "Appearing offline";
  return PRESENCE_LABELS[me.presence];
}
