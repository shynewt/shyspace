import { ChevronRight, FileText, Hash, Plus, Search, Users } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useMemo, useRef, useState } from "react";
import { api, type Page, type SyncStatus } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { sameView, useWorkspace, type View } from "../../lib/workspace";
import { cx, IconButton, Kbd, Logo, SectionLabel, springs } from "../../ui";
import { SyncIndicator } from "./SyncIndicator";

const INDENT_PX = 14;

export function Sidebar({
  status,
  onOpenSearch,
  onOpenMembers,
}: {
  status: SyncStatus | undefined;
  onOpenSearch: () => void;
  onOpenMembers: () => void;
}) {
  const { session, channels, pages, navigate } = useWorkspace();
  const [isAddingChannel, setAddingChannel] = useState(false);

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
          <div className="truncate text-[14px] font-semibold tracking-tight">{session.workspace_name}</div>
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
          <Kbd>Ctrl K</Kbd>
        </button>
      </div>

      <nav aria-label="Workspace" className="flex-1 overflow-y-auto px-2 pb-3">
        <Section title="Channels" addLabel="New channel" onAdd={() => setAddingChannel(true)}>
          {channels.map((channel) => (
            <NavItem
              key={channel.id}
              view={{ kind: "channel", id: channel.id }}
              icon={<Hash size={15} />}
              label={channel.name}
            />
          ))}
          <AnimatePresence>
            {isAddingChannel && <NewChannelInput onDone={() => setAddingChannel(false)} />}
          </AnimatePresence>
        </Section>

        <Section title="Pages" addLabel="New page" onAdd={() => createPage(null)}>
          <PageTree pages={pages} parent={null} depth={0} onCreateChild={createPage} />
        </Section>
      </nav>

      <div className="border-t border-line p-2">
        <button
          type="button"
          onClick={onOpenMembers}
          className="flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-[13px] text-muted transition hover:bg-surface-2 hover:text-ink"
        >
          <Users size={15} />
          <span className="flex-1 text-left">Members{session.is_admin ? " & invites" : ""}</span>
        </button>
      </div>
    </aside>
  );
}

function Section({
  title,
  addLabel,
  onAdd,
  children,
}: {
  title: string;
  addLabel: string;
  onAdd: () => void;
  children: ReactNode;
}) {
  return (
    <div className="mt-3">
      <SectionLabel
        action={
          <IconButton
            label={addLabel}
            size="sm"
            icon={<Plus size={14} />}
            onClick={onAdd}
            className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
          />
        }
      >
        {title}
      </SectionLabel>
      <div className="flex flex-col gap-px">{children}</div>
    </div>
  );
}

function NavItem({
  view,
  icon,
  label,
  depth = 0,
  leading,
  trailing,
}: {
  view: View;
  icon: ReactNode;
  label: string;
  depth?: number;
  /** Rendered before the item, outside its button (like a fold toggle). */
  leading?: ReactNode;
  trailing?: ReactNode;
}) {
  const { view: current, navigate } = useWorkspace();
  const isActive = sameView(current, view);
  return (
    <div className="group relative flex items-center" style={{ paddingLeft: depth * INDENT_PX }}>
      {isActive && (
        <motion.div
          layoutId="nav-active"
          transition={springs.snappy}
          className="absolute inset-y-0 right-0 rounded-lg bg-surface shadow-card"
          style={{ left: depth * INDENT_PX }}
        />
      )}
      {leading}
      <button
        type="button"
        aria-current={isActive ? "page" : undefined}
        onClick={() => navigate(view)}
        className={cx(
          "relative flex h-8 min-w-0 flex-1 items-center gap-2 rounded-lg px-2.5 text-[13.5px] transition-colors",
          isActive ? "font-medium text-ink" : "text-muted hover:bg-surface-2/70 hover:text-ink",
        )}
      >
        <span className={isActive ? "text-accent" : "text-faint"}>{icon}</span>
        <span className="truncate">{label}</span>
      </button>
      {trailing}
    </div>
  );
}

function PageTree({
  pages,
  parent,
  depth,
  onCreateChild,
}: {
  pages: Page[];
  parent: string | null;
  depth: number;
  onCreateChild: (parent: string) => void;
}) {
  const children = useMemo(() => pages.filter((page) => page.parent === parent), [pages, parent]);
  return (
    <>
      {children.map((page) => (
        <PageNode key={page.id} page={page} pages={pages} depth={depth} onCreateChild={onCreateChild} />
      ))}
    </>
  );
}

function PageNode({
  page,
  pages,
  depth,
  onCreateChild,
}: {
  page: Page;
  pages: Page[];
  depth: number;
  onCreateChild: (parent: string) => void;
}) {
  const [isOpen, setOpen] = useState(true);
  const hasChildren = pages.some((candidate) => candidate.parent === page.id);
  const title = page.title || "Untitled";
  return (
    <div>
      <NavItem
        view={{ kind: "page", id: page.id }}
        depth={depth}
        label={title}
        icon={<FileText size={15} />}
        leading={
          hasChildren && (
            <button
              type="button"
              aria-expanded={isOpen}
              aria-label={`${isOpen ? "Collapse" : "Expand"} ${title}`}
              onClick={() => setOpen(!isOpen)}
              className="relative -mr-1 grid h-6 w-5 shrink-0 place-items-center rounded text-faint hover:text-ink"
            >
              <motion.span animate={{ rotate: isOpen ? 90 : 0 }} transition={springs.snappy} className="grid">
                <ChevronRight size={14} />
              </motion.span>
            </button>
          )
        }
        trailing={
          <IconButton
            label="New sub-page"
            size="sm"
            icon={<Plus size={13} />}
            onClick={() => onCreateChild(page.id)}
            className="absolute right-1.5 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
          />
        }
      />
      <AnimatePresence initial={false}>
        {hasChildren && isOpen && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
            className="overflow-hidden"
          >
            <PageTree pages={pages} parent={page.id} depth={depth + 1} onCreateChild={onCreateChild} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function NewChannelInput({ onDone }: { onDone: () => void }) {
  const { navigate } = useWorkspace();
  const [name, setName] = useState("");
  // Enter submits and closes the input, and closing it fires blur, which
  // submits too. Without this guard the channel was created twice.
  const isSubmitted = useRef(false);
  const input = useRef<HTMLInputElement>(null);
  useFocusOnMount(input);
  const submit = async () => {
    if (isSubmitted.current) return;
    isSubmitted.current = true;
    if (!name.trim()) return onDone();
    try {
      navigate({ kind: "channel", id: await api.createChannel(name) });
    } catch (error) {
      reportError(error);
    }
    onDone();
  };
  return (
    <motion.div
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: "auto" }}
      exit={{ opacity: 0, height: 0 }}
    >
      <div className="flex h-8 items-center gap-2 rounded-lg bg-surface px-2.5 ring-2 ring-accent/30">
        <Hash size={15} className="text-accent" />
        <input
          ref={input}
          aria-label="Channel name"
          value={name}
          onChange={(event) => setName(event.target.value.toLowerCase().replace(/\s+/g, "-"))}
          onKeyDown={(event) => {
            if (event.key === "Enter") submit();
            if (event.key === "Escape") onDone();
          }}
          onBlur={submit}
          placeholder="channel-name"
          className="min-w-0 flex-1 bg-transparent text-[13.5px] outline-none placeholder:text-faint"
        />
      </div>
    </motion.div>
  );
}
