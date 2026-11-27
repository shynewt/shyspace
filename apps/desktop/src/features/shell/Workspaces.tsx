import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { useAlertActions } from "../../lib/alertActions";
import {
  api,
  type Change,
  type LaterItem,
  onAnyWorkspaceChange,
  onLinkEnded,
  type Session,
  type WorkspaceBadge,
  type WorkspaceItem,
  type WorkspaceLook,
} from "../../lib/api";
import { saveEverythingBeforeSwitch } from "../../lib/beforeSwitch";
import { useAskToNotifyOnce } from "../../lib/notificationPermission";
import { shownView } from "../../lib/shownView";
import { dismissUndoToasts, reportError, showToast } from "../../lib/toast";
import { useAlerts } from "../../lib/useAlerts";
import { useLive } from "../../lib/useLive";
import { usePresence } from "../../lib/usePresence";
import { useReminders } from "../../lib/useReminders";
import { savedItemView, type View } from "../../lib/workspace";
import { useDeviceNoticeAlerts } from "../devices/useDeviceNoticeAlerts";

type WorkspacesValue = {
  /** Every workspace on this device, in rail order. */
  workspaces: WorkspaceItem[];
  badges: ReadonlyMap<string, WorkspaceBadge>;
  shownId: string;
  /** The shown workspace's default theme and accent; null until loaded. */
  look: WorkspaceLook | null;
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
  return ["channels", "messages", "alert", "page_mentions", "later", "workspaces"].includes(change.kind);
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
  onLook,
  children,
}: {
  session: Session;
  /** The session to show next; null when the last workspace was left. */
  onSession: (session: Session | null) => void;
  onAdd: () => void;
  /** The shown workspace's look, to apply to the page; null once no workspace is shown. */
  onLook: (look: WorkspaceLook | null) => void;
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

  // A link a restart interrupted before its hand-over goes on in Add workspace, once per start.
  const hasCheckedWaitingLink = useRef(false);
  useEffect(() => {
    if (hasCheckedWaitingLink.current) return;
    hasCheckedWaitingLink.current = true;
    api.waitingDeviceLink().then((waiting) => waiting && onAdd(), reportError);
  }, [onAdd]);

  // A linking device's link ended without it being added: its workspace is gone, so say why and show what's left.
  useEffect(
    () =>
      onLinkEnded((message) => {
        reportError(message);
        api.session().then(onSession, reportError);
      }),
    [onSession],
  );

  // A linking device's reading-only workspace was replaced by the real one: show that one.
  useEffect(
    () =>
      onAnyWorkspaceChange((workspaceId, change) => {
        if (change.kind !== "reading_closed" || workspaceId !== session.workspace_id) return;
        api.session().then(onSession, reportError);
      }),
    [session.workspace_id, onSession],
  );

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
  useDeviceNoticeAlerts(useMemo(() => workspaces.map((workspace) => workspace.workspace_id), [workspaces]));
  useAlertActions(
    useMemo(() => workspaces.map((workspace) => workspace.workspace_id).join(" "), [workspaces]),
    openIn,
  );
  useAskToNotifyOnce();
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

  const [look, setLook] = useState<WorkspaceLook | null>(null);
  useEffect(() => onLook(look), [look, onLook]);
  useEffect(() => () => onLook(null), [onLook]);

  const value = useMemo(
    () => ({ workspaces, badges, shownId: session.workspace_id, look, select, openIn, leave, add: onAdd }),
    [workspaces, badges, session.workspace_id, look, select, openIn, leave, onAdd],
  );
  return (
    <WorkspacesContext.Provider value={value}>
      <ShownWorkspaceLook key={session.workspace_id} onLook={setLook} />
      {children}
    </WorkspacesContext.Provider>
  );
}

const loadLook = () => api.workspaceLook();
const isLookChange = (change: Change) => change.kind === "workspace_look";

/** Loads the shown workspace's look, kept current; keyed by workspace, so a switch loads the next one's. */
function ShownWorkspaceLook({ onLook }: { onLook: (look: WorkspaceLook) => void }) {
  const look = useLive(loadLook, isLookChange);
  useEffect(() => {
    if (look) onLook(look);
  }, [look, onLook]);
  return null;
}
