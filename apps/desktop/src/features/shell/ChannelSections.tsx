import { ArrowDownAZ, ChevronRight, Clock, Ellipsis, FolderPlus, ListFilter, Pencil, Plus, Trash2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useRef, useState } from "react";
import { api, type SectionSort, type SidebarSection } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { useLive } from "../../lib/useLive";
import { ConfirmDialog, cx, IconButton, Menu, type MenuItem, springs } from "../../ui";
import { ChannelList } from "./ChannelList";

const SORTS: { sort: SectionSort; label: string; icon: ReactNode }[] = [
  { sort: "name", label: "A to Z", icon: <ArrowDownAZ size={14} /> },
  { sort: "recent", label: "Recent activity", icon: <Clock size={14} /> },
];

/** The channels, grouped into your personal sidebar sections. */
export function ChannelSections() {
  const sections = useLive(api.sidebarSections, (change) => change.kind === "channels" || change.kind === "messages");
  const [isUnreadOnly, setUnreadOnly] = useState(false);
  const [isAddingSection, setAddingSection] = useState(false);
  // Each opening of the new-channel field is a fresh one: reopening while
  // the last one fades out must not revive it, already submitted.
  const [newChannelRound, setNewChannelRound] = useState<number | null>(null);
  const roundsStarted = useRef(0);
  if (!sections) return null;
  return (
    <>
      {sections.map((section) => (
        <Section
          key={section.id}
          section={section}
          sections={sections}
          isUnreadOnly={isUnreadOnly}
          newChannelRound={section.is_builtin ? newChannelRound : null}
          onAddDone={() => setNewChannelRound(null)}
          builtinActions={
            section.is_builtin && (
              <>
                <IconButton
                  label={isUnreadOnly ? "Show all channels" : "Show unread channels only"}
                  aria-pressed={isUnreadOnly}
                  size="sm"
                  icon={<ListFilter size={13} />}
                  isActive={isUnreadOnly}
                  onClick={() => setUnreadOnly(!isUnreadOnly)}
                  className={cx(!isUnreadOnly && "opacity-0 group-hover/section:opacity-100 focus-visible:opacity-100")}
                />
                <IconButton
                  label="New channel"
                  size="sm"
                  icon={<Plus size={14} />}
                  onClick={() => {
                    roundsStarted.current += 1;
                    setNewChannelRound(roundsStarted.current);
                  }}
                  className="opacity-0 group-hover/section:opacity-100 focus-visible:opacity-100"
                />
              </>
            )
          }
          onAddSection={() => setAddingSection(true)}
        />
      ))}
      <AnimatePresence>
        {isAddingSection && (
          <SectionNameInput
            label="Section name"
            initialName=""
            onSubmit={(name) => api.createSection(name)}
            onDone={() => setAddingSection(false)}
          />
        )}
      </AnimatePresence>
    </>
  );
}

function Section({
  section,
  sections,
  isUnreadOnly,
  newChannelRound,
  onAddDone,
  builtinActions,
  onAddSection,
}: {
  section: SidebarSection;
  sections: SidebarSection[];
  isUnreadOnly: boolean;
  newChannelRound: number | null;
  onAddDone: () => void;
  builtinActions: ReactNode;
  onAddSection: () => void;
}) {
  const [isRenaming, setRenaming] = useState(false);
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
  const isOpen = !section.is_collapsed;
  const items: MenuItem[] = [
    {
      label: "Sort",
      icon: <ArrowDownAZ size={14} />,
      choices: SORTS.map((choice) => ({
        label: choice.label,
        icon: choice.icon,
        isChosen: section.sort === choice.sort,
        onSelect: () => api.setSectionSort(section.id, choice.sort).catch(reportError),
      })),
    },
    { label: "New section", icon: <FolderPlus size={14} />, onSelect: onAddSection },
  ];
  if (!section.is_builtin) {
    items.unshift({ label: "Rename section", icon: <Pencil size={14} />, onSelect: () => setRenaming(true) });
    items.push({
      label: "Delete section",
      icon: <Trash2 size={14} />,
      onSelect: () => setConfirmingDelete(true),
      isDestructive: true,
    });
  }
  return (
    <section aria-label={section.name} className="mt-3">
      {isRenaming ? (
        <SectionNameInput
          label={`New name for ${section.name}`}
          initialName={section.name}
          onSubmit={(name) => api.renameSection(section.id, name)}
          onDone={() => setRenaming(false)}
        />
      ) : (
        <div className="group/section flex h-7 items-center gap-0.5 pr-1 pl-1">
          <button
            type="button"
            aria-expanded={isOpen}
            onClick={() => api.setSectionCollapsed(section.id, isOpen).catch(reportError)}
            className="flex min-w-0 flex-1 items-center gap-1 rounded px-1 text-[11px] font-semibold tracking-wide text-faint uppercase hover:text-muted"
          >
            <motion.span animate={{ rotate: isOpen ? 90 : 0 }} transition={springs.snappy} className="grid">
              <ChevronRight size={12} />
            </motion.span>
            <span className="truncate">{section.name}</span>
          </button>
          {builtinActions}
          <span className="opacity-0 group-hover/section:opacity-100 keyboard-focus-within:opacity-100">
            <Menu
              trigger={
                <IconButton label={`Actions for the ${section.name} section`} size="sm" icon={<Ellipsis size={13} />} />
              }
              items={items}
            />
          </span>
        </div>
      )}
      <AnimatePresence initial={false}>
        {isOpen && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
            className="flex flex-col gap-px overflow-hidden"
          >
            <ChannelList
              channelIds={section.channels}
              sections={sections}
              newChannelRound={newChannelRound}
              onAddDone={onAddDone}
              isUnreadOnly={isUnreadOnly}
            />
          </motion.div>
        )}
      </AnimatePresence>
      <ConfirmDialog
        open={isConfirmingDelete}
        onOpenChange={setConfirmingDelete}
        title={`Delete the ${section.name} section?`}
        description="Its channels move back to Channels. Only you see your sections."
        confirmLabel="Delete section"
        onConfirm={() => api.deleteSection(section.id)}
      />
    </section>
  );
}

/** An inline field for a section's name: Enter or leaving submits, Escape cancels. */
function SectionNameInput({
  label,
  initialName,
  onSubmit,
  onDone,
}: {
  label: string;
  initialName: string;
  onSubmit: (name: string) => Promise<unknown>;
  onDone: () => void;
}) {
  const [name, setName] = useState(initialName);
  const isSubmitted = useRef(false);
  const input = useRef<HTMLInputElement>(null);
  useFocusOnMount(input);
  const submit = async () => {
    if (isSubmitted.current) return;
    isSubmitted.current = true;
    if (name.trim() && name !== initialName) await onSubmit(name).catch(reportError);
    onDone();
  };
  return (
    <motion.div
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: "auto" }}
      exit={{ opacity: 0, height: 0 }}
    >
      <input
        ref={input}
        aria-label={label}
        value={name}
        onChange={(event) => setName(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") submit();
          if (event.key === "Escape") {
            isSubmitted.current = true;
            onDone();
          }
        }}
        onBlur={submit}
        placeholder="Section name"
        className="mt-3 h-7 w-full rounded-lg bg-surface px-2.5 text-[12px] font-semibold tracking-wide uppercase ring-2 ring-accent/30 outline-none placeholder:text-faint placeholder:normal-case"
      />
    </motion.div>
  );
}
