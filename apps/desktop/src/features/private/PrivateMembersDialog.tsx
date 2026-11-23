import { Crown, Ellipsis, LogOut, UserMinus, UserPlus } from "lucide-react";
import { useCallback, useState } from "react";
import { api, type Channel, type Member } from "../../lib/api";
import { distinctName } from "../../lib/people";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Badge, Button, ConfirmDialog, Dialog, IconButton, Menu, Spinner } from "../../ui";
import { MemberRows } from "../members/MemberList";
import { MemberPicker, useOtherMembers } from "./MemberPicker";

/** Who is in a private channel. Its owner adds, removes and hands over; anyone can leave. */
export function PrivateMembersDialog({
  channel,
  open,
  onOpenChange,
}: {
  channel: Channel;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={`People in #${channel.name}`}
      description={
        channel.is_member
          ? "Only they can read this channel. Someone you add can read what was said before."
          : "You're no longer in this channel. This is who was in it when you left."
      }
      width={520}
    >
      <PrivateMembers channel={channel} onLeft={() => onOpenChange(false)} />
    </Dialog>
  );
}

/** A choice that needs a yes first. */
type Pending = { kind: "remove" | "hand_over"; member: Member } | { kind: "leave" };

function PrivateMembers({ channel, onLeft }: { channel: Channel; onLeft: () => void }) {
  const { session } = useWorkspace();
  const loadPrivate = useCallback(() => api.privateMembers(channel.id), [channel.id]);
  const inChannel = useLive(loadPrivate, (change) => change.kind === "channels" || change.kind === "members");
  const others = useOtherMembers();
  const everyone = useLive(api.members, (change) => change.kind === "members");
  const [addable, setAddable] = useState<Member[] | null>(null);
  const [added, setAdded] = useState<ReadonlySet<string>>(new Set());
  const [adding, setAdding] = useState<ReadonlySet<string>>(new Set());
  const [pending, setPending] = useState<Pending | null>(null);
  if (!inChannel || !others || !everyone) {
    return (
      <div className="flex justify-center py-4 text-decorative">
        <Spinner size={16} />
      </div>
    );
  }

  const ids = new Set(inChannel.members);
  const invited = new Set(inChannel.invited);
  const members = everyone.filter((member) => ids.has(member.id));
  const isOwner = inChannel.is_owner && channel.is_member;
  const isAlone = inChannel.members.length <= 1;
  const add = async (member: Member) => {
    if (added.has(member.id) || adding.has(member.id)) return;
    setAdding(new Set([...adding, member.id]));
    try {
      await api.addToPrivateChannel(channel.id, member.id);
      setAdded((current) => new Set([...current, member.id]));
    } catch (error) {
      reportError(error);
    } finally {
      setAdding((current) => new Set([...current].filter((id) => id !== member.id)));
    }
  };
  const nameOf = (member: Member) => distinctName(member, everyone ?? []);

  return (
    <div className="flex flex-col gap-4">
      {addable ? (
        <MemberPicker label="People to add" members={addable} chosen={added} busy={adding} onToggle={add} />
      ) : (
        <MemberRows
          label="People in this channel"
          members={members}
          actions={(member) => (
            <>
              {member.id === inChannel.owner && <Badge tone="accent">Owner</Badge>}
              {invited.has(member.id) && <Badge>Invited</Badge>}
              {isOwner && member.id !== session.me && (
                <Menu
                  trigger={
                    <IconButton label={`Actions for ${nameOf(member)}`} size="sm" icon={<Ellipsis size={13} />} />
                  }
                  items={[
                    {
                      label: "Make owner",
                      icon: <Crown size={14} />,
                      onSelect: () => setPending({ kind: "hand_over", member }),
                    },
                    {
                      label: "Remove from channel",
                      icon: <UserMinus size={14} />,
                      onSelect: () => setPending({ kind: "remove", member }),
                      isDestructive: true,
                    },
                  ]}
                />
              )}
            </>
          )}
        />
      )}
      {!addable && invited.size > 0 && (
        <p className="text-xs text-muted">
          Invited people haven't read anything here yet, so they may not have its key.
        </p>
      )}
      {channel.is_member && (
        <div className="flex items-center justify-between gap-2">
          {!isOwner || isAlone ? (
            <Button variant="ghost" icon={<LogOut size={14} />} onClick={() => setPending({ kind: "leave" })}>
              Leave channel
            </Button>
          ) : (
            <p className="text-xs text-muted">To leave, make someone else the owner first.</p>
          )}
          {isOwner && (
            <Button
              variant={addable ? "secondary" : "primary"}
              icon={addable ? undefined : <UserPlus size={14} />}
              onClick={() => {
                setAdded(new Set());
                setAddable(addable ? null : others.filter((member) => !ids.has(member.id)));
              }}
            >
              {addable ? "Done adding" : "Add people"}
            </Button>
          )}
        </div>
      )}
      <PendingConfirm
        pending={pending}
        channel={channel}
        nameOf={nameOf}
        isLastOne={isAlone}
        onClose={() => setPending(null)}
        onLeft={onLeft}
      />
    </div>
  );
}

function PendingConfirm({
  pending,
  channel,
  nameOf,
  isLastOne,
  onClose,
  onLeft,
}: {
  pending: Pending | null;
  channel: Channel;
  nameOf: (member: Member) => string;
  isLastOne: boolean;
  onClose: () => void;
  onLeft: () => void;
}) {
  const close = (isOpen: boolean) => !isOpen && onClose();
  if (pending?.kind === "remove") {
    return (
      <ConfirmDialog
        open
        onOpenChange={close}
        title={`Remove ${nameOf(pending.member)} from #${channel.name}?`}
        description="They keep what they've already read, but can't read anything new or write here."
        confirmLabel="Remove"
        onConfirm={() => api.removeFromPrivateChannel(channel.id, pending.member.id)}
      />
    );
  }
  if (pending?.kind === "hand_over") {
    return (
      <ConfirmDialog
        open
        onOpenChange={close}
        title={`Make ${nameOf(pending.member)} the owner?`}
        description="They'll add and remove people from now on, and you won't be able to unless they hand it back."
        confirmLabel="Make owner"
        onConfirm={() => api.handOverPrivateChannel(channel.id, pending.member.id)}
      />
    );
  }
  if (pending?.kind === "leave") {
    return (
      <ConfirmDialog
        open
        onOpenChange={close}
        title={`Leave #${channel.name}?`}
        description={
          isLastOne
            ? "You're the last one in it, so nobody will be able to read or add to it again."
            : "You keep what you've read, but won't see anything new unless the owner adds you back."
        }
        confirmLabel="Leave channel"
        onConfirm={async () => {
          await api.leavePrivateChannel(channel.id);
          onLeft();
        }}
      />
    );
  }
  return null;
}
