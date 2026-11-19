import { useCallback, useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { UserX } from "lucide-react";
import { api, type Session } from "../lib/api";
import { useLive } from "../lib/useLive";
import { type View, WorkspaceContext } from "../lib/workspace";
import { Sidebar } from "./Sidebar";
import { ChannelView } from "./ChannelView";
import { PageView } from "./PageView";
import { CommandPalette } from "./CommandPalette";
import { MembersPanel } from "./MembersPanel";

export function Shell({ session }: { session: Session }) {
  const channels = useLive(api.channels, [], (change) => change.kind === "channels") ?? [];
  const pages = useLive(api.pages, [], (change) => change.kind === "pages") ?? [];
  const status = useLive(api.status, [], (change) => change.kind === "sync" || change.kind === "removed");
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
  const context = useMemo(() => ({ session, channels, pages, view, navigate }), [session, channels, pages, view, navigate]);

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
      <CommandPalette open={isPaletteOpen} onClose={() => setPaletteOpen(false)} />
      <MembersPanel open={isMembersOpen} onClose={() => setMembersOpen(false)} />
      <AnimatePresence>{status?.removed && <RemovedNotice />}</AnimatePresence>
    </WorkspaceContext.Provider>
  );
}

function RemovedNotice() {
  return (
    <motion.div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 backdrop-blur-sm"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
    >
      <motion.div
        className="max-w-sm rounded-2xl border border-line bg-surface p-6 text-center shadow-float"
        initial={{ scale: 0.92, y: 10 }}
        animate={{ scale: 1, y: 0 }}
      >
        <div className="mx-auto grid h-11 w-11 place-items-center rounded-xl bg-danger/12 text-danger">
          <UserX size={20} />
        </div>
        <h2 className="mt-3 font-semibold">You were removed from this workspace</h2>
        <p className="mt-1 text-[13px] text-muted">
          You can still read what you already had, but new messages and edits are encrypted with keys you no longer
          receive.
        </p>
      </motion.div>
    </motion.div>
  );
}
