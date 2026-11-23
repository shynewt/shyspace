import { UserX } from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Session } from "../../lib/api";
import { useMembersWithCurrentStatus } from "../../lib/members";
import type { SettingsTarget } from "../../lib/settings";
import { SHORTCUTS } from "../../lib/shortcuts";
import { showView } from "../../lib/shownView";
import { useLive } from "../../lib/useLive";
import { useMouseBackAndForward } from "../../lib/useMouseBackAndForward";
import { ScrollMemoryContext, useNavigation } from "../../lib/useNavigation";
import { sameView, sidebarViews, type View, viewKey, WorkspaceContext } from "../../lib/workspace";
import { Dialog, tweens } from "../../ui";
import { ChannelView } from "../chat/ChannelView";
import { DraftsAndSentView } from "../drafts/DraftsAndSentView";
import { DriveView } from "../drive/DriveView";
import { HuddleProvider } from "../huddles/HuddleContext";
import { InboxView } from "../inbox/InboxView";
import { LaterView } from "../later/LaterView";
import { FullProfileDialog } from "../members/FullProfileDialog";
import { PageView } from "../pages/PageView";
import { CommandPalette } from "../search/CommandPalette";
import { SettingsDialog } from "../settings/SettingsDialog";
import { ShellLayout } from "./ShellLayout";
import { ShortcutsDialog } from "./ShortcutsDialog";
import { Sidebar } from "./Sidebar";
import { StatusDialog } from "./StatusDialog";
import { useSidebar } from "./useSidebar";
import { ViewHeader } from "./ViewHeader";
import { WorkspaceRail } from "./WorkspaceRail";
import { takePendingOpen, useWorkspaces } from "./Workspaces";

export function Shell({ session }: { session: Session }) {
  // Unread counts change as messages arrive.
  const loadedChannels = useLive(api.channels, (change) => change.kind === "channels" || change.kind === "messages");
  const loadedPages = useLive(api.pages, (change) => change.kind === "pages");
  const channels = useMemo(() => loadedChannels ?? [], [loadedChannels]);
  const pages = useMemo(() => loadedPages ?? [], [loadedPages]);
  const sections = useLive(api.sidebarSections, (change) => change.kind === "channels" || change.kind === "messages");
  const loadedMembers = useLive(api.members, (change) => change.kind === "members");
  const members = useMembersWithCurrentStatus(loadedMembers);
  const status = useLive(api.status, (change) => change.kind === "sync" || change.kind === "removed");
  const { entry, restore, navigate, back, forward, replace, scrollMemory } = useNavigation(
    session.workspace_id,
    session.me,
    loadedChannels,
    loadedPages,
  );
  const view = entry?.view ?? null;
  const sidebar = useSidebar();
  const spaces = useWorkspaces();
  /** What the search palette opens with; null while it's closed. */
  const [paletteQuery, setPaletteQuery] = useState<string | null>(null);
  const [isStatusOpen, setStatusOpen] = useState(false);
  const [shownProfile, setShownProfile] = useState<{ memberId: string; returnFocusTo: HTMLElement | null } | null>(
    null,
  );
  const [isShortcutsOpen, setShortcutsOpen] = useState(false);
  const [settingsTarget, setSettingsTarget] = useState<SettingsTarget | null>(null);

  // Leave a channel or page once it's deleted. Only one that was listed
  // before counts: a brand-new one may not be listed yet.
  const listedIds = useMemo(() => new Set([...channels, ...pages].map((item) => item.id)), [channels, pages]);
  const previouslyListedIds = useRef(listedIds);
  useEffect(() => {
    const id = view && (view.kind === "channel" || view.kind === "page") ? view.id : null;
    if (id && previouslyListedIds.current.has(id) && !listedIds.has(id)) {
      replace(channels.length > 0 ? { kind: "channel", id: channels[0].id } : null);
    }
    previouslyListedIds.current = listedIds;
  }, [view, listedIds, channels, replace]);

  const { toggle: toggleSidebar } = sidebar;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      // Keys something focused already used, like the page editor's Alt+↓, stay theirs.
      if (event.defaultPrevented) return;
      const shortcut = SHORTCUTS.find((candidate) => candidate.matches(event));
      // The huddle's keys belong to HuddleProvider, which knows your call.
      if (!shortcut || shortcut.id === "huddle" || shortcut.id === "huddleMute") return;
      event.preventDefault();
      const channelOrder = sections?.flatMap((section) => section.channels) ?? channels.map((channel) => channel.id);
      const views = sidebarViews(channelOrder, pages);
      const current = view ? views.findIndex((candidate) => sameView(candidate, view)) : -1;
      const unread = (candidate: View) =>
        candidate.kind === "channel" && (channels.find((channel) => channel.id === candidate.id)?.unread ?? 0) > 0;
      const step = (direction: 1 | -1, isWanted: (candidate: View) => boolean) => {
        for (let index = current + direction; index >= 0 && index < views.length; index += direction) {
          if (isWanted(views[index])) return navigate(views[index]);
        }
      };
      switch (shortcut.id) {
        case "search":
          return setPaletteQuery((query) => (query === null ? "" : null));
        case "inbox":
          return navigate({ kind: "inbox" });
        case "draftsAndSent":
          return navigate({ kind: "drafts" });
        case "back":
          return back();
        case "forward":
          return forward();
        case "previous":
          return step(-1, () => true);
        case "next":
          return step(1, () => true);
        case "previousUnread":
          return step(-1, unread);
        case "nextUnread":
          return step(1, unread);
        case "cheatSheet":
          return setShortcutsOpen(true);
        case "settings":
          return setSettingsTarget((current) => current ?? { section: "profile" });
        case "toggleSidebar":
          return toggleSidebar();
        case "workspace": {
          const chosen = spaces.workspaces[Number(event.code.slice("Digit".length)) - 1];
          if (chosen) spaces.select(chosen.workspace_id);
          return;
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [view, channels, pages, sections, navigate, back, forward, toggleSidebar, spaces]);

  useMouseBackAndForward(back, forward);

  // What notifications above the shell need: where you are, and how to open something here.
  const currentView = useRef(view);
  currentView.current = view;
  useEffect(
    () => showView({ workspaceId: session.workspace_id, view: () => currentView.current, navigate }),
    [session.workspace_id, navigate],
  );
  // What a notification or reminder from this workspace asked to open while another was shown.
  const [openFirst] = useState(takePendingOpen);
  const hasOpenedFirst = useRef(false);
  useEffect(() => {
    if (!openFirst || hasOpenedFirst.current) return;
    hasOpenedFirst.current = true;
    openFirst(navigate);
  }, [openFirst, navigate]);

  const openSettings = useCallback((target: SettingsTarget = { section: "profile" }) => setSettingsTarget(target), []);
  const openSearch = useCallback((query = "") => setPaletteQuery(query), []);
  const openStatus = useCallback(() => setStatusOpen(true), []);
  const openProfile = useCallback(
    (memberId: string, returnFocusTo: HTMLElement | null = null) => setShownProfile({ memberId, returnFocusTo }),
    [],
  );
  const { leaveDrawer } = sidebar;
  // Choosing somewhere to go is what the drawer is for, so it closes; changes from elsewhere (a delete) leave it.
  const navigateFromScreen = useCallback(
    (target: View) => {
      leaveDrawer();
      navigate(target);
    },
    [leaveDrawer, navigate],
  );
  const context = useMemo(
    () => ({
      session,
      channels,
      pages,
      members,
      view,
      navigate: navigateFromScreen,
      openSettings,
      openSearch,
      openStatus,
      openProfile,
    }),
    [session, channels, pages, members, view, navigateFromScreen, openSettings, openSearch, openStatus, openProfile],
  );
  const me = members?.find((member) => member.id === session.me);

  return (
    <WorkspaceContext.Provider value={context}>
      <HuddleProvider>
        <ShellLayout
          sidebar={sidebar}
          sidebarContent={<Sidebar status={status} onOpenSearch={() => openSearch()} />}
          rail={spaces.workspaces.length > 1 ? <WorkspaceRail /> : null}
        >
          {/* No exit animation: switching while a view fades in could leave AnimatePresence waiting forever on an exit, with nothing shown. */}
          {view && (
            <motion.div
              key={viewKey(view)}
              className="absolute inset-0"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={tweens.fast}
            >
              {view.kind === "channel" && (
                <ScrollMemoryContext.Provider value={scrollMemory}>
                  <ChannelView channelId={view.id} thread={view.thread} focus={view.focus} restore={restore} />
                </ScrollMemoryContext.Provider>
              )}
              {view.kind === "page" && <PageView pageId={view.id} />}
              {view.kind === "inbox" && <InboxView />}
              {view.kind === "later" && <LaterView />}
              {view.kind === "files" && <DriveView />}
              {view.kind === "drafts" && <DraftsAndSentView />}
            </motion.div>
          )}
          {!view && !sidebar.isDocked && <ViewHeader />}
        </ShellLayout>
        <CommandPalette
          open={paletteQuery !== null}
          initialQuery={paletteQuery ?? ""}
          onOpenChange={(open) => setPaletteQuery(open ? (paletteQuery ?? "") : null)}
        />
        <StatusDialog open={isStatusOpen} onOpenChange={setStatusOpen} current={me?.status ?? null} />
        <FullProfileDialog
          memberId={shownProfile?.memberId ?? null}
          returnFocusTo={shownProfile?.returnFocusTo ?? null}
          onClose={() => setShownProfile(null)}
        />
        <ShortcutsDialog open={isShortcutsOpen} onOpenChange={setShortcutsOpen} />
        <SettingsDialog
          target={settingsTarget}
          onNavigate={setSettingsTarget}
          onClose={() => setSettingsTarget(null)}
        />
        <RemovedNotice isRemoved={status?.removed ?? false} />
      </HuddleProvider>
    </WorkspaceContext.Provider>
  );
}

function RemovedNotice({ isRemoved }: { isRemoved: boolean }) {
  return (
    <Dialog
      open={isRemoved}
      onOpenChange={() => {}}
      title="You were removed from this workspace"
      description="You can still read what you already had, but new messages and edits use keys you no longer receive."
      width={380}
    >
      <div className="flex justify-center">
        <div className="grid h-11 w-11 place-items-center rounded-xl bg-danger/12 text-danger">
          <UserX size={20} />
        </div>
      </div>
    </Dialog>
  );
}
