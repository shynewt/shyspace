import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useState } from "react";
import {
  api,
  type Change,
  type LaterItem,
  onAnyWorkspaceChange,
  type Session,
  type WorkspaceBadge,
  type WorkspaceItem,
} from "../../lib/api";
import { saveEverythingBeforeSwitch } from "../../lib/beforeSwitch";
import { shownView } from "../../lib/shownView";
import { dismissUndoToasts, reportError, showToast } from "../../lib/toast";
import { useAlerts } from "../../lib/useAlerts";
import { usePresence } from "../../lib/usePresence";
import { useReminders } from "../../lib/useReminders";
import { savedItemView, type View } from "../../lib/workspace";

type WorkspacesValue = {
  /** Every workspace on this device, in rail order. */
  workspaces: WorkspaceItem[];
  badges: ReadonlyMap<string, WorkspaceBadge>;
  shownId: string;
  /** Shows another workspace, then runs `open` there if given (a notification's message). */
  select: (workspaceId: string, open?: Opener) => void;
  /** Opens something in `workspaceId`: right away if it's shown, else after showing it. */
  openIn: (workspaceId: string, open: Opener) => void;
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

/** Opens something in the shown workspace's screen, given how to navigate there. */
export type Opener = (navigate: (view: View) => void) => void;

/** What to open once a workspace has been shown (a notification's message), read once by the next shell. */
let pendingOpen: Opener | null = null;

export function takePendingOpen(): Opener | null {
  const open = pendingOpen;
  pendingOpen = null;
  return open;
}

/** Changes that move a workspace's unread or mention counts. */
function changesBadges(change: Change): boolean {
  return ["channels", "messages", "alert", "later", "workspaces"].includes(change.kind);
}

/**
 * The workspaces on this device, for the rail, notifications and menus: kept
 * current from every workspace's changes, not only the shown one's. Mounted
 * whenever a workspace exists, including while adding another, so
 * notifications keep coming.
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
    (workspaceId: string, open?: Opener) => {
      if (workspaceId === session.workspace_id) {
        // Already selected, as while adding another: show it again, opening there.
        pendingOpen = open ?? null;
        onSession(session);
        return;
      }
      dismissUndoToasts();
      saveEverythingBeforeSwitch()
        .then(() => api.selectWorkspace(workspaceId))
        .then((next) => {
          pendingOpen = open ?? null;
          onSession(next);
        }, reportError);
    },
    [session, onSession],
  );
  const openIn = useCallback(
    (workspaceId: string, open: Opener) => {
      const shown = shownView();
      if (shown?.workspaceId === workspaceId) return open(shown.navigate);
      select(workspaceId, open);
    },
    [select],
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

  const alertWorkspaces = useMemo(
    () => ({ names: new Map(workspaces.map((workspace) => [workspace.workspace_id, workspace.name])), openIn }),
    [workspaces, openIn],
  );
  useAlerts(alertWorkspaces);
  usePresence();
  useReminders(
    useCallback(
      (workspaceId: string, item: LaterItem) =>
        openIn(workspaceId, (navigate) => {
          const saved = savedItemView(item);
          if (saved) navigate(saved);
          else showToast("What you saved no longer exists.", "info");
        }),
      [openIn],
    ),
  );
  // The window's badge counts what needs you in every workspace on this device.
  const attention = [...badges.values()].reduce((total, badge) => total + badge.attention, 0);
  useEffect(() => {
    api.setAttentionCount(attention).catch(reportError);
  }, [attention]);

  const value = useMemo(
    () => ({ workspaces, badges, shownId: session.workspace_id, select, openIn, leave, add: onAdd }),
    [workspaces, badges, session.workspace_id, select, openIn, leave, onAdd],
  );
  return <WorkspacesContext.Provider value={value}>{children}</WorkspacesContext.Provider>;
}
