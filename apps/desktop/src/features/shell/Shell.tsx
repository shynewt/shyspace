import { UserX } from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type LaterItem, type Session } from "../../lib/api";
import { useMembersWithCurrentStatus } from "../../lib/members";
import type { SettingsTarget } from "../../lib/settings";
import { SHORTCUTS } from "../../lib/shortcuts";
import { showToast } from "../../lib/toast";
import { useAlerts } from "../../lib/useAlerts";
import { useLive } from "../../lib/useLive";
import { useMouseBackAndForward } from "../../lib/useMouseBackAndForward";
import { ScrollMemoryContext, useNavigation } from "../../lib/useNavigation";
import { usePresence } from "../../lib/usePresence";
import { useReminders } from "../../lib/useReminders";
import { sameView, savedItemView, sidebarViews, type View, viewKey, WorkspaceContext } from "../../lib/workspace";
import { Dialog } from "../../ui";
import { ChannelView } from "../chat/ChannelView";
import { InboxView } from "../inbox/InboxView";
import { LaterView } from "../later/LaterView";
import { PageView } from "../pages/PageView";
import { CommandPalette } from "../search/CommandPalette";
import { SettingsDialog } from "../settings/SettingsDialog";
import { ShortcutsDialog } from "./ShortcutsDialog";
import { Sidebar } from "./Sidebar";
import { useAttentionBadge } from "./useAttentionBadge";

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
    session.me,
    loadedChannels,
    loadedPages,
  );
  const view = entry?.view ?? null;
  const [isPaletteOpen, setPaletteOpen] = useState(false);
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

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      // Keys something focused already used, like the page editor's Alt+↓, stay theirs.
      if (event.defaultPrevented) return;
      const shortcut = SHORTCUTS.find((candidate) => candidate.matches(event));
      if (!shortcut) return;
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
          return setPaletteOpen((open) => !open);
        case "inbox":
          return navigate({ kind: "inbox" });
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
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [view, channels, pages, sections, navigate, back, forward]);

  useMouseBackAndForward(back, forward);

  const openSaved = useCallback(
    (item: LaterItem) => {
      const target = savedItemView(item);
      if (target) navigate(target);
      else showToast("What you saved no longer exists.", "info");
    },
    [navigate],
  );

  useAlerts(view, navigate);
  useReminders(openSaved);
  usePresence();

  const openSettings = useCallback((target: SettingsTarget = { section: "profile" }) => setSettingsTarget(target), []);
  const context = useMemo(
    () => ({ session, channels, pages, members, view, navigate, openSettings }),
    [session, channels, pages, members, view, navigate, openSettings],
  );

  return (
    <WorkspaceContext.Provider value={context}>
      <AttentionBadge />
      <motion.div
        className="flex h-full"
        initial={{ opacity: 0, scale: 0.985 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.35, ease: [0.2, 0.8, 0.2, 1] }}
      >
        <Sidebar status={status} onOpenSearch={() => setPaletteOpen(true)} />
        <main className="relative min-w-0 flex-1 bg-surface">
          {/* No exit animation: switching while a view fades in could leave AnimatePresence waiting forever on an exit, with nothing shown. */}
          {view && (
            <motion.div
              key={viewKey(view)}
              className="absolute inset-0"
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.2, ease: "easeOut" }}
            >
              {view.kind === "channel" && (
                <ScrollMemoryContext.Provider value={scrollMemory}>
                  <ChannelView channelId={view.id} thread={view.thread} focus={view.focus} restore={restore} />
                </ScrollMemoryContext.Provider>
              )}
              {view.kind === "page" && <PageView pageId={view.id} />}
              {view.kind === "inbox" && <InboxView />}
              {view.kind === "later" && <LaterView />}
            </motion.div>
          )}
        </main>
      </motion.div>
      <CommandPalette open={isPaletteOpen} onOpenChange={setPaletteOpen} />
      <ShortcutsDialog open={isShortcutsOpen} onOpenChange={setShortcutsOpen} />
      <SettingsDialog target={settingsTarget} onNavigate={setSettingsTarget} onClose={() => setSettingsTarget(null)} />
      <RemovedNotice isRemoved={status?.removed ?? false} />
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
      <div className="flex justify-center px-5 pt-4 pb-5">
        <div className="grid h-11 w-11 place-items-center rounded-xl bg-danger/12 text-danger">
          <UserX size={20} />
        </div>
      </div>
    </Dialog>
  );
}

/** Keeps the count of what needs you showing outside the window. */
function AttentionBadge() {
  useAttentionBadge();
  return null;
}
