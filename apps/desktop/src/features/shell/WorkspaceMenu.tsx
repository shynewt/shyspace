import { ChevronDown, Settings, UserCheck, UserPlus } from "lucide-react";
import { useWorkspace } from "../../lib/workspace";
import { Menu, type MenuItem } from "../../ui";
import { JoinRequestsBadge, useJoinRequests } from "../members/JoinRequests";

/** The workspace's name at the top of the sidebar, opening what concerns the whole workspace. */
export function WorkspaceMenu() {
  const { session } = useWorkspace();
  return session.is_admin ? <AdminWorkspaceMenu /> : <WorkspaceMenuButton items={[]} waitingRequests={0} />;
}

function AdminWorkspaceMenu() {
  const { openSettings } = useWorkspace();
  const waiting = useJoinRequests()?.length ?? 0;
  const items: MenuItem[] = [
    {
      label: "Invite people",
      icon: <UserPlus size={14} />,
      onSelect: () => openSettings({ section: "members", membersTab: "invites" }),
    },
  ];
  if (waiting > 0) {
    items.unshift({
      label: `Join requests (${waiting})`,
      icon: <UserCheck size={14} />,
      onSelect: () => openSettings({ section: "members", membersTab: "requests" }),
    });
  }
  return <WorkspaceMenuButton items={items} waitingRequests={waiting} />;
}

function WorkspaceMenuButton({ items, waitingRequests }: { items: MenuItem[]; waitingRequests: number }) {
  const { session, openSettings } = useWorkspace();
  return (
    <Menu
      items={[
        ...items,
        {
          label: "Workspace settings",
          icon: <Settings size={14} />,
          onSelect: () => openSettings({ section: "members" }),
        },
      ]}
      trigger={
        <button
          type="button"
          className="-mx-1.5 flex w-[calc(100%+0.75rem)] min-w-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-left transition-colors hover:bg-surface-2 data-[state=open]:bg-surface-2"
        >
          <span className="truncate text-[14px] font-semibold tracking-tight">{session.workspace_name}</span>
          <ChevronDown size={14} className="shrink-0 text-faint" />
          <JoinRequestsBadge count={waitingRequests} />
        </button>
      }
    />
  );
}
