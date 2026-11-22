import { Bell, BellMinus, BellOff, Ellipsis, FolderInput, Hash, Lock, Pencil, Trash2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useRef, useState } from "react";
import { api, type Channel, type NotifyLevel, type SidebarSection } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { useLive } from "../../lib/useLive";
import { useWorkspace, type View } from "../../lib/workspace";
import { ConfirmDialog, cx, IconButton, Menu, type MenuItem, tweens } from "../../ui";
import { useHuddleSummary } from "../huddles/HuddleMarker";
import { NavItem } from "./NavItem";

const NOTIFY_CHOICES: { level: NotifyLevel; label: string; icon: ReactNode }[] = [
  { level: "all", label: "Every message", icon: <Bell size={14} /> },
  { level: "mentions", label: "Mentions only", icon: <BellMinus size={14} /> },
  { level: "nothing", label: "Nothing (muted)", icon: <BellOff size={14} /> },
];

/** Shown next to a channel's name when it doesn't alert for every message. */
const LEVEL_STATUS: Record<NotifyLevel, { icon: ReactNode; label: string } | null> = {
  all: null,
  mentions: { icon: <BellMinus size={12} />, label: "mentions only" },
  nothing: { icon: <BellOff size={12} />, label: "muted" },
};
const DRAFT_STATUS = { icon: <Pencil size={12} />, label: "draft" };

/** A section's channels as the sidebar lists them: all, or only unread ones plus the open one. */
export function listedChannels(
  channelIds: string[],
  channels: Channel[],
  view: View | null,
  isUnreadOnly: boolean,
): Channel[] {
  // DMs and notes have their own place.
  const inSection = channelIds.flatMap((id) =>
    channels.filter((channel) => channel.id === id && (channel.kind === "public" || channel.kind === "private")),
  );
  if (!isUnreadOnly) return inSection;
  // The open channel stays listed, so filtering never hides where you are.
  return inSection.filter((channel) => channel.unread > 0 || (view?.kind === "channel" && view.id === channel.id));
}

export function ChannelList({
  channelIds,
  sections,
  newChannelRound,
  onAddDone,
  isUnreadOnly,
}: {
  /** The section's channels, in its order. */
  channelIds: string[];
  /** Every section, for "Move to". */
  sections: SidebarSection[];
  /** Set while the new-channel field is open; a new number means a fresh field. */
  newChannelRound: number | null;
  onAddDone: () => void;
  isUnreadOnly: boolean;
}) {
  const { channels, view, navigate } = useWorkspace();
  const drafts = useLive(api.drafts, (change) => change.kind === "drafts");
  const hasDraft = (channel: string) => drafts?.some((draft) => draft.target === channel) ?? false;
  const shown = listedChannels(channelIds, channels, view, isUnreadOnly);
  const create = async (name: string) => navigate({ kind: "channel", id: await api.createChannel(name) });
  return (
    <>
      {shown.map((channel) => (
        <ChannelItem key={channel.id} channel={channel} hasDraft={hasDraft(channel.id)} sections={sections} />
      ))}
      <AnimatePresence>
        {newChannelRound !== null && (
          <ChannelNameInput
            key={newChannelRound}
            label="Channel name"
            initialName=""
            onSubmit={create}
            onDone={onAddDone}
          />
        )}
      </AnimatePresence>
    </>
  );
}

function ChannelItem({
  channel,
  hasDraft,
  sections,
}: {
  channel: Channel;
  hasDraft: boolean;
  sections: SidebarSection[];
}) {
  const [isRenaming, setRenaming] = useState(false);
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
  const [isMenuOpen, setMenuOpen] = useState(false);
  const huddle = useHuddleSummary(channel.id);
  if (isRenaming) {
    return (
      <ChannelNameInput
        label={`New name for #${channel.name}`}
        initialName={channel.name}
        onSubmit={(name) => api.renameChannel(channel.id, name)}
        onDone={() => setRenaming(false)}
      />
    );
  }
  const setLevel = (level: NotifyLevel) => api.setNotifyLevel(channel.id, level).catch(reportError);
  const items: MenuItem[] = [
    { label: "Rename", icon: <Pencil size={14} />, onSelect: () => setRenaming(true) },
    {
      label: "Notifications",
      icon: <Bell size={14} />,
      choices: NOTIFY_CHOICES.map((choice) => ({
        label: choice.label,
        icon: choice.icon,
        isChosen: channel.notify === choice.level,
        onSelect: () => setLevel(choice.level),
      })),
    },
    {
      label: "Move to",
      icon: <FolderInput size={14} />,
      choices: sections.map((section) => ({
        label: section.name,
        isChosen: section.channels.includes(channel.id),
        onSelect: () => api.moveChannel(channel.id, section.is_builtin ? null : section.id).catch(reportError),
      })),
    },
  ];
  if (channel.can_delete) {
    items.push({
      label: "Delete channel",
      icon: <Trash2 size={14} />,
      onSelect: () => setConfirmingDelete(true),
      isDestructive: true,
    });
  }
  return (
    <>
      <NavItem
        view={{ kind: "channel", id: channel.id }}
        icon={channel.kind === "private" ? <Lock size={15} /> : <Hash size={15} />}
        label={channel.name}
        kind={channel.kind === "private" ? "private channel" : undefined}
        unread={channel.unread}
        mentions={channel.unread_mentions}
        isMuted={channel.notify === "nothing"}
        status={hasDraft ? DRAFT_STATUS : LEVEL_STATUS[channel.notify]}
        huddle={huddle}
        onContextMenu={() => setMenuOpen(true)}
        trailing={
          // Takes room only while shown, so a long name ends before it instead of under it.
          <span
            className={cx(
              "relative grid shrink-0 justify-items-start overflow-hidden transition-opacity",
              isMenuOpen
                ? "w-7.5 opacity-100"
                : "w-0 opacity-0 group-hover:w-7.5 group-hover:opacity-100 keyboard-focus-within:w-7.5 keyboard-focus-within:opacity-100",
            )}
          >
            <Menu
              trigger={<IconButton label={`Actions for #${channel.name}`} size="sm" icon={<Ellipsis size={13} />} />}
              items={items}
              open={isMenuOpen}
              onOpenChange={setMenuOpen}
            />
          </span>
        }
      />
      <ConfirmDialog
        open={isConfirmingDelete}
        onOpenChange={setConfirmingDelete}
        title={`Delete #${channel.name}?`}
        description="The channel and every message in it disappear for everyone. This can't be undone."
        confirmLabel="Delete channel"
        onConfirm={() => api.deleteChannel(channel.id)}
      />
    </>
  );
}

/** An inline name field: Enter or leaving the field submits, Escape cancels. */
function ChannelNameInput({
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
  // Enter submits and closes the input, and closing it fires blur, which
  // submits too. Without this guard the channel was created twice.
  const isSubmitted = useRef(false);
  const input = useRef<HTMLInputElement>(null);
  useFocusOnMount(input);
  const submit = async () => {
    if (isSubmitted.current) return;
    isSubmitted.current = true;
    if (!name.trim() || name === initialName) return onDone();
    try {
      await onSubmit(name);
    } catch (error) {
      reportError(error);
    }
    onDone();
  };
  return (
    <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={tweens.fast}>
      <div className="flex h-8 items-center gap-2 rounded-lg bg-surface px-2.5 ring-2 ring-accent/30">
        <Hash size={15} className="text-accent" />
        <input
          ref={input}
          aria-label={label}
          value={name}
          onChange={(event) => setName(event.target.value.toLowerCase().replace(/\s+/g, "-"))}
          onKeyDown={(event) => {
            if (event.key === "Enter") submit();
            if (event.key === "Escape") {
              // Used here: the channel behind mustn't take it as "mark read".
              event.preventDefault();
              isSubmitted.current = true;
              onDone();
            }
          }}
          onBlur={submit}
          placeholder="channel-name"
          className="min-w-0 flex-1 bg-transparent text-base outline-none placeholder:text-faint"
        />
      </div>
    </motion.div>
  );
}
