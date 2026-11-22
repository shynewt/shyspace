import { Bookmark, HardDrive, Inbox as InboxIcon, Search, Send } from "lucide-react";
import { AnimatePresence } from "motion/react";
import { useCallback } from "react";
import { api, type SyncStatus } from "../../lib/api";
import { shortcutKeys } from "../../lib/shortcuts";
import { useLive } from "../../lib/useLive";
import { Kbd, Logo } from "../../ui";
import { HuddleBar } from "../huddles/HuddleBar";
import { useMyHuddle } from "../huddles/HuddleContext";
import { ChannelSections } from "./ChannelSections";
import { DirectMessages } from "./DirectMessages";
import { MeMenu } from "./MeMenu";
import { NavItem } from "./NavItem";
import { PagesSection } from "./PagesSection";
import { SidebarToggle } from "./ShellLayout";
import { SyncIndicator } from "./SyncIndicator";
import { WorkspaceMenu } from "./WorkspaceMenu";

/** The sidebar's content; `ShellLayout` places it, docked or as a drawer. */
export function Sidebar({ status, onOpenSearch }: { status: SyncStatus | undefined; onOpenSearch: () => void }) {
  const { mine } = useMyHuddle();
  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2.5 pt-4 pr-2.5 pb-3 pl-4">
        <Logo size={26} />
        <div className="min-w-0 flex-1">
          <WorkspaceMenu />
          <SyncIndicator status={status} />
        </div>
        <SidebarToggle placement="sidebar" />
      </div>

      <div className="px-3 pb-2">
        <button
          type="button"
          onClick={onOpenSearch}
          className="flex h-8 w-full items-center gap-2 rounded-lg border border-line bg-surface px-2.5 text-sm text-muted transition hover:border-surface-3 hover:text-ink"
        >
          <Search size={14} />
          <span className="flex-1 text-left">Search</span>
          <Kbd>{shortcutKeys("search").join(" ")}</Kbd>
        </button>
      </div>

      <nav aria-label="Workspace" className="flex-1 overflow-y-auto px-2 pb-3">
        <InboxLink />
        <LaterLink />
        <DraftsLink />
        <NavItem view={{ kind: "files" }} icon={<HardDrive size={15} />} label="Files" />
        <ChannelSections />
        <DirectMessages />

        <PagesSection />
      </nav>

      <AnimatePresence initial={false}>{mine && <HuddleBar key="huddle" huddle={mine} />}</AnimatePresence>
      <div className="border-t border-line p-2">
        <MeMenu />
      </div>
    </div>
  );
}

/** The inbox, counting mentions and new replies in threads you follow. */
function InboxLink() {
  const loadInbox = useCallback(() => api.inbox(), []);
  const inbox = useLive(loadInbox, (change) => change.kind === "messages" || change.kind === "channels");
  return (
    <NavItem
      view={{ kind: "inbox" }}
      icon={<InboxIcon size={15} />}
      label="Inbox"
      unread={inbox?.attention.length ?? 0}
    />
  );
}

/** Later, counting what's saved and not done yet. */
function LaterLink() {
  const later = useLive(api.later, (change) => change.kind === "later" || change.kind === "messages");
  // A reminder comes due exactly when the core hands it out, which also
  // reloads this list, so checking the clock here is enough.
  const now = Date.now();
  const due = later?.filter((item) => !item.is_done && item.remind_ms !== null && item.remind_ms <= now).length ?? 0;
  return <NavItem view={{ kind: "later" }} icon={<Bookmark size={15} />} label="Later" unread={due} countNoun="due" />;
}

/** Drafts & sent, quietly counting drafts: they wait for you, they don't need you. */
function DraftsLink() {
  const drafts = useLive(api.drafts, (change) => change.kind === "drafts");
  const count = drafts?.length ?? 0;
  return (
    <NavItem
      view={{ kind: "drafts" }}
      icon={<Send size={15} />}
      label="Drafts & sent"
      waiting={{ count, label: `${count} ${count === 1 ? "draft" : "drafts"}` }}
    />
  );
}
