import { Bookmark, Inbox as InboxIcon, Plus, Search } from "lucide-react";
import { type ReactNode, useCallback } from "react";
import { api, type SyncStatus } from "../../lib/api";
import { shortcutKeys } from "../../lib/shortcuts";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { IconButton, Kbd, Logo, SectionLabel } from "../../ui";
import { ChannelSections } from "./ChannelSections";
import { MeMenu } from "./MeMenu";
import { NavItem } from "./NavItem";
import { PageTree } from "./PageTree";
import { SyncIndicator } from "./SyncIndicator";
import { WorkspaceMenu } from "./WorkspaceMenu";

export function Sidebar({ status, onOpenSearch }: { status: SyncStatus | undefined; onOpenSearch: () => void }) {
  const { pages, navigate } = useWorkspace();

  const createPage = async (parent: string | null) => {
    try {
      navigate({ kind: "page", id: await api.createPage("", parent) });
    } catch (error) {
      reportError(error);
    }
  };

  return (
    <aside className="flex w-[264px] shrink-0 flex-col border-r border-line bg-bg">
      <div className="flex items-center gap-2.5 px-4 pt-4 pb-3">
        <Logo size={26} />
        <div className="min-w-0 flex-1">
          <WorkspaceMenu />
          <SyncIndicator status={status} />
        </div>
      </div>

      <div className="px-3 pb-2">
        <button
          type="button"
          onClick={onOpenSearch}
          className="flex h-8 w-full items-center gap-2 rounded-lg border border-line bg-surface px-2.5 text-[13px] text-muted transition hover:border-surface-3 hover:text-ink"
        >
          <Search size={14} />
          <span className="flex-1 text-left">Search</span>
          <Kbd>{shortcutKeys("search").join(" ")}</Kbd>
        </button>
      </div>

      <nav aria-label="Workspace" className="flex-1 overflow-y-auto px-2 pb-3">
        <InboxLink />
        <LaterLink />
        <ChannelSections />

        <Section title="Pages" addLabel="New page" onAdd={() => createPage(null)}>
          <PageTree pages={pages} parent={null} depth={0} onCreateChild={createPage} />
        </Section>
      </nav>

      <div className="border-t border-line p-2">
        <MeMenu />
      </div>
    </aside>
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

function Section({
  title,
  addLabel,
  onAdd,
  extraAction,
  children,
}: {
  title: string;
  addLabel: string;
  onAdd: () => void;
  extraAction?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="mt-3">
      <SectionLabel
        action={
          <span className="flex items-center">
            {extraAction}
            <IconButton
              label={addLabel}
              size="sm"
              icon={<Plus size={14} />}
              onClick={onAdd}
              className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
            />
          </span>
        }
      >
        {title}
      </SectionLabel>
      <div className="flex flex-col gap-px">{children}</div>
    </div>
  );
}
