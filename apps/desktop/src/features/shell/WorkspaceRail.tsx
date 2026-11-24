import { ArrowDown, ArrowUp, Bell, BellOff, CheckCheck, LogOut, Plus } from "lucide-react";
import { useState } from "react";
import { api, type WorkspaceBadge, type WorkspaceItem } from "../../lib/api";
import { useWorkspaceIcon } from "../../lib/files";
import { shortcutKeys } from "../../lib/shortcuts";
import { reportError } from "../../lib/toast";
import {
  Avatar,
  ConfirmDialog,
  ContextMenu,
  CountBadge,
  cx,
  IconButton,
  Kbd,
  type MenuAction,
  Tooltip,
} from "../../ui";
import { useWorkspaces } from "./Workspaces";

/**
 * Every workspace on this device, as a column of icons beside the sidebar,
 * shown once there are two or more. The shown one has an accent bar; others
 * show a dot for unread and a count for mentions, nothing while muted.
 * Ctrl/⌘+1…9 picks one by place; a right-click mutes, reads, moves or leaves.
 */
export function WorkspaceRail() {
  const { workspaces, badges, add } = useWorkspaces();
  const [leaving, setLeaving] = useState<WorkspaceItem | null>(null);
  return (
    <nav aria-label="Workspaces" className="flex w-rail shrink-0 flex-col items-center gap-2 border-r border-line py-3">
      {workspaces.map((workspace, index) => (
        <RailItem
          key={workspace.workspace_id}
          workspace={workspace}
          badge={badges.get(workspace.workspace_id)}
          position={index}
          count={workspaces.length}
          onLeave={() => setLeaving(workspace)}
        />
      ))}
      <IconButton label="Add a workspace" icon={<Plus size={16} />} onClick={add} className="mt-1" />
      <LeaveWorkspaceDialog workspace={leaving} onClose={() => setLeaving(null)} />
    </nav>
  );
}

function RailItem({
  workspace,
  badge,
  position,
  count,
  onLeave,
}: {
  workspace: WorkspaceItem;
  badge: WorkspaceBadge | undefined;
  position: number;
  count: number;
  onLeave: () => void;
}) {
  const { shownId, select } = useWorkspaces();
  const id = workspace.workspace_id;
  const iconUrl = useWorkspaceIcon(id, workspace.icon);
  const isShown = id === shownId;
  const mentions = workspace.is_muted || isShown ? 0 : (badge?.mentions ?? 0);
  const hasUnread = !workspace.is_muted && !isShown && Boolean(badge?.unread);
  const keys = position < 9 ? [shortcutKeys("workspace")[0], String(position + 1)] : null;
  const items: MenuAction[] = [
    workspace.is_muted
      ? {
          label: "Unmute notifications",
          icon: <Bell size={14} />,
          onSelect: () => api.setWorkspaceMuted(id, false).catch(reportError),
        }
      : {
          label: "Mute notifications",
          icon: <BellOff size={14} />,
          onSelect: () => api.setWorkspaceMuted(id, true).catch(reportError),
        },
    {
      label: "Mark all as read",
      icon: <CheckCheck size={14} />,
      onSelect: () => api.markWorkspaceRead(id).catch(reportError),
    },
    ...(position > 0
      ? [
          {
            label: "Move up",
            icon: <ArrowUp size={14} />,
            onSelect: () => api.moveWorkspace(id, position - 1).catch(reportError),
          },
        ]
      : []),
    ...(position < count - 1
      ? [
          {
            label: "Move down",
            icon: <ArrowDown size={14} />,
            onSelect: () => api.moveWorkspace(id, position + 1).catch(reportError),
          },
        ]
      : []),
    { label: "Leave workspace…", icon: <LogOut size={14} />, isDestructive: true, onSelect: onLeave },
  ];
  const state = [
    workspace.is_muted && "muted",
    mentions > 0 && `${mentions} ${mentions === 1 ? "mention" : "mentions"}`,
    hasUnread && mentions === 0 && "unread",
  ].filter(Boolean);
  return (
    <ContextMenu items={items}>
      <div className="relative flex w-full justify-center">
        {/* The edge says where you are (a bar) or that something's unread there (a dot). */}
        {isShown && <span aria-hidden className="absolute top-1.5 left-0 h-5 w-1 rounded-r-full bg-accent" />}
        {hasUnread && <span aria-hidden className="absolute top-3.5 left-0 h-1.5 w-1 rounded-r-full bg-ink" />}
        <Tooltip
          side="right"
          label={
            <span className="flex items-center gap-2">
              {workspace.name}
              {keys && <Kbd>{keys.join(" ")}</Kbd>}
            </span>
          }
        >
          <button
            type="button"
            aria-label={[workspace.name, ...state].join(", ")}
            aria-current={isShown ? "page" : undefined}
            onClick={() => select(id)}
            className={cx(
              "relative rounded-[34%] transition-opacity",
              isShown
                ? "opacity-100"
                : workspace.is_muted
                  ? "opacity-40 hover:opacity-70"
                  : "opacity-80 hover:opacity-100",
            )}
          >
            <Avatar id={id} name={workspace.name} size={32} pictureUrl={iconUrl} />
            {mentions > 0 && (
              <CountBadge tone="danger" className="absolute -top-1.5 -right-2 ring-2 ring-bg">
                {mentions > 99 ? "99+" : mentions}
              </CountBadge>
            )}
          </button>
        </Tooltip>
      </div>
    </ContextMenu>
  );
}

/** Asks before deleting a workspace from this device, saying what goes. */
export function LeaveWorkspaceDialog({ workspace, onClose }: { workspace: WorkspaceItem | null; onClose: () => void }) {
  const { leave } = useWorkspaces();
  return (
    <ConfirmDialog
      open={workspace !== null}
      onOpenChange={(open) => !open && onClose()}
      title={`Leave ${workspace?.name ?? "this workspace"}?`}
      description="This device stops syncing it, and its messages, pages and files are removed from this device. To come back, you'll need a new invite."
      confirmLabel="Leave workspace"
      onConfirm={async () => {
        if (workspace) await leave(workspace.workspace_id);
      }}
    />
  );
}
