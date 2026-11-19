import { AtSign, Bell, BellOff, Ellipsis, Hash, Pencil, Trash2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useRef, useState } from "react";
import { api, type Channel, type NotifyLevel } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { ConfirmDialog, cx, IconButton, Menu, type MenuItem } from "../../ui";
import { NavItem } from "./NavItem";

const NOTIFY_CHOICES: { level: NotifyLevel; label: string; icon: ReactNode }[] = [
  { level: "all", label: "Every message", icon: <Bell size={14} /> },
  { level: "mentions", label: "Mentions only", icon: <AtSign size={14} /> },
  { level: "nothing", label: "Nothing (muted)", icon: <BellOff size={14} /> },
];

/** Shown next to a channel's name when it doesn't alert for every message. */
const LEVEL_STATUS: Record<NotifyLevel, { icon: ReactNode; label: string } | null> = {
  all: null,
  mentions: { icon: <AtSign size={12} />, label: "mentions only" },
  nothing: { icon: <BellOff size={12} />, label: "muted" },
};
const DRAFT_STATUS = { icon: <Pencil size={12} />, label: "draft" };

export function ChannelList({
  newChannelRound,
  onAddDone,
  isUnreadOnly,
}: {
  /** Set while the new-channel field is open; a new number means a fresh field. */
  newChannelRound: number | null;
  onAddDone: () => void;
  isUnreadOnly: boolean;
}) {
  const { channels, view, navigate } = useWorkspace();
  const drafts = useLive(api.drafts, (change) => change.kind === "drafts");
  const hasDraft = (channel: string) => drafts?.some((draft) => draft.target === channel) ?? false;
  // The open channel stays listed, so filtering never hides where you are.
  const shown = isUnreadOnly
    ? channels.filter((channel) => channel.unread > 0 || (view?.kind === "channel" && view.id === channel.id))
    : channels;
  const create = async (name: string) => navigate({ kind: "channel", id: await api.createChannel(name) });
  return (
    <>
      {shown.map((channel) => (
        <ChannelItem key={channel.id} channel={channel} hasDraft={hasDraft(channel.id)} />
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

function ChannelItem({ channel, hasDraft }: { channel: Channel; hasDraft: boolean }) {
  const [isRenaming, setRenaming] = useState(false);
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
  const [isMenuOpen, setMenuOpen] = useState(false);
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
        icon={<Hash size={15} />}
        label={channel.name}
        unread={channel.unread}
        mentions={channel.unread_mentions}
        isMuted={channel.notify === "nothing"}
        status={hasDraft ? DRAFT_STATUS : LEVEL_STATUS[channel.notify]}
        onContextMenu={() => setMenuOpen(true)}
        trailing={
          <span
            className={cx(
              "absolute right-1.5 group-hover:opacity-100 focus-within:opacity-100",
              isMenuOpen ? "opacity-100" : "opacity-0",
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
    <motion.div
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: "auto" }}
      exit={{ opacity: 0, height: 0 }}
    >
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
              isSubmitted.current = true;
              onDone();
            }
          }}
          onBlur={submit}
          placeholder="channel-name"
          className="min-w-0 flex-1 bg-transparent text-[13.5px] outline-none placeholder:text-faint"
        />
      </div>
    </motion.div>
  );
}
