import { UserX } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type Session } from "../../lib/api";
import { useLive } from "../../lib/useLive";
import { type View, WorkspaceContext } from "../../lib/workspace";
import { Dialog } from "../../ui";
import { ChannelView } from "../chat/ChannelView";
import { MembersPanel } from "../members/MembersPanel";
import { PageView } from "../pages/PageView";
import { CommandPalette } from "../search/CommandPalette";
import { Sidebar } from "./Sidebar";

export function Shell({ session }: { session: Session }) {
  const channels = useLive(api.channels, (change) => change.kind === "channels") ?? [];
  const pages = useLive(api.pages, (change) => change.kind === "pages") ?? [];
  const status = useLive(api.status, (change) => change.kind === "sync" || change.kind === "removed");
  const [view, setView] = useState<View | null>(null);
  const [isPaletteOpen, setPaletteOpen] = useState(false);
  const [isMembersOpen, setMembersOpen] = useState(false);

  useEffect(() => {
    if (!view && channels.length > 0) setView({ kind: "channel", id: channels[0].id });
  }, [view, channels]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const navigate = useCallback((next: View) => setView(next), []);
  const context = useMemo(
    () => ({ session, channels, pages, view, navigate }),
    [session, channels, pages, view, navigate],
  );

  return (
    <WorkspaceContext.Provider value={context}>
      <motion.div
        className="flex h-full"
        initial={{ opacity: 0, scale: 0.985 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.35, ease: [0.2, 0.8, 0.2, 1] }}
      >
        <Sidebar status={status} onOpenSearch={() => setPaletteOpen(true)} onOpenMembers={() => setMembersOpen(true)} />
        <main className="relative min-w-0 flex-1 bg-surface">
          <AnimatePresence mode="wait" initial={false}>
            {view && (
              <motion.div
                key={`${view.kind}:${view.id}`}
                className="absolute inset-0"
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, transition: { duration: 0.1 } }}
                transition={{ duration: 0.2, ease: "easeOut" }}
              >
                {view.kind === "channel" ? <ChannelView channelId={view.id} /> : <PageView pageId={view.id} />}
              </motion.div>
            )}
          </AnimatePresence>
        </main>
      </motion.div>
      <CommandPalette open={isPaletteOpen} onOpenChange={setPaletteOpen} />
      <MembersPanel open={isMembersOpen} onOpenChange={setMembersOpen} />
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
