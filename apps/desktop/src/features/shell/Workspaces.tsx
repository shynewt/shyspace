import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useState } from "react";
import {
  type AlertTarget,
  api,
  type Change,
  onAnyWorkspaceChange,
  type Session,
  type WorkspaceBadge,
  type WorkspaceItem,
} from "../../lib/api";
import { saveEverythingBeforeSwitch } from "../../lib/beforeSwitch";
import { reportError } from "../../lib/toast";

type WorkspacesValue = {
  /** Every workspace on this device, in rail order. */
  workspaces: WorkspaceItem[];
  badges: ReadonlyMap<string, WorkspaceBadge>;
  shownId: string;
  /** Shows another workspace, then opens `target` in it if given (a notification's message). */
  select: (workspaceId: string, target?: AlertTarget) => void;
  /** Deletes a workspace from this device (the caller asks first). */
  leave: (workspaceId: string) => Promise<void>;
  /** Opens joining, creating or restoring another workspace. */
  add: () => void;
};

const WorkspacesContext = createContext<WorkspacesValue | null>(null);

export function useWorkspaces(): WorkspacesValue {
  const value = useContext(WorkspacesContext);
  if (!value) throw new Error("useWorkspaces needs a WorkspacesProvider");
  return value;
}

/** What a notification opens once its workspace has been shown, read once by the next shell. */
let pendingTarget: AlertTarget | null = null;

export function takePendingTarget(): AlertTarget | null {
  const target = pendingTarget;
  pendingTarget = null;
  return target;
}

/** Changes that move a workspace's unread or mention counts. */
function changesBadges(change: Change): boolean {
  return ["channels", "messages", "alert", "workspaces"].includes(change.kind);
}

/**
 * The workspaces on this device, for the rail, notifications and menus: kept
 * current from every workspace's changes, not only the shown one's.
 */
export function WorkspacesProvider({
  session,
  onSession,
  onAdd,
  children,
}: {
  session: Session;
  /** The session to show next; null when the last workspace was left. */
  onSession: (session: Session | null) => void;
  onAdd: () => void;
  children: ReactNode;
}) {
  const [workspaces, setWorkspaces] = useState<WorkspaceItem[]>([]);
  const [badges, setBadges] = useState<ReadonlyMap<string, WorkspaceBadge>>(new Map());

  useEffect(() => {
    let isCurrent = true;
    const loadWorkspaces = () => api.workspaces().then((loaded) => isCurrent && setWorkspaces(loaded), reportError);
    const loadBadges = () =>
      api
        .workspaceBadges()
        .then(
          (loaded) => isCurrent && setBadges(new Map(loaded.map((badge) => [badge.workspace_id, badge]))),
          reportError,
        );
    loadWorkspaces();
    loadBadges();
    const stop = onAnyWorkspaceChange((_, change) => {
      if (change.kind === "workspaces") loadWorkspaces();
      if (changesBadges(change)) loadBadges();
    });
    return () => {
      isCurrent = false;
      stop();
    };
  }, []);

  const select = useCallback(
    (workspaceId: string, target?: AlertTarget) => {
      if (workspaceId === session.workspace_id) return;
      saveEverythingBeforeSwitch()
        .then(() => api.selectWorkspace(workspaceId))
        .then((next) => {
          pendingTarget = target ?? null;
          onSession(next);
        }, reportError);
    },
    [session.workspace_id, onSession],
  );
  const leave = useCallback(
    async (workspaceId: string) => {
      try {
        const next = await api.leaveWorkspace(workspaceId);
        if (workspaceId === session.workspace_id || next === null) onSession(next);
      } catch (error) {
        // A leave that failed partway may have changed which workspace is shown; show that one, then say why.
        const now = await api.session();
        if (now?.workspace_id !== session.workspace_id) onSession(now);
        throw error;
      }
    },
    [session.workspace_id, onSession],
  );

  const value = useMemo(
    () => ({ workspaces, badges, shownId: session.workspace_id, select, leave, add: onAdd }),
    [workspaces, badges, session.workspace_id, select, leave, onAdd],
  );
  return <WorkspacesContext.Provider value={value}>{children}</WorkspacesContext.Provider>;
}
