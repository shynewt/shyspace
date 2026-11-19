import { Ellipsis, Hash, Pencil, Trash2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useRef, useState } from "react";
import { api, type Channel } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { useWorkspace } from "../../lib/workspace";
import { ConfirmDialog, IconButton, Menu, type MenuItem } from "../../ui";
import { NavItem } from "./NavItem";

export function ChannelList({
  isAdding,
  onAddDone,
  isUnreadOnly,
}: {
  isAdding: boolean;
  onAddDone: () => void;
  isUnreadOnly: boolean;
}) {
  const { channels, view, navigate } = useWorkspace();
  // The open channel stays listed, so filtering never hides where you are.
  const shown = isUnreadOnly
    ? channels.filter((channel) => channel.unread > 0 || (view?.kind === "channel" && view.id === channel.id))
    : channels;
  const create = async (name: string) => navigate({ kind: "channel", id: await api.createChannel(name) });
  return (
    <>
      {shown.map((channel) => (
        <ChannelItem key={channel.id} channel={channel} />
      ))}
      <AnimatePresence>
        {isAdding && <ChannelNameInput label="Channel name" initialName="" onSubmit={create} onDone={onAddDone} />}
      </AnimatePresence>
    </>
  );
}

function ChannelItem({ channel }: { channel: Channel }) {
  const [isRenaming, setRenaming] = useState(false);
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
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
  const items: MenuItem[] = [{ label: "Rename", icon: <Pencil size={14} />, onSelect: () => setRenaming(true) }];
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
        trailing={
          <span className="absolute right-1.5 opacity-0 group-hover:opacity-100 focus-within:opacity-100">
            <Menu
              trigger={<IconButton label={`Actions for #${channel.name}`} size="sm" icon={<Ellipsis size={13} />} />}
              items={items}
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
