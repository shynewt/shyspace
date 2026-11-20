import { Crown, Ellipsis, LogOut, UserMinus, UserPlus } from "lucide-react";
import { useCallback, useState } from "react";
import { api, type Channel, type Member } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Badge, Button, ConfirmDialog, Dialog, IconButton, Menu } from "../../ui";
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
      description="Only they can read this channel. Someone you add can read what was said before."
      width={520}
    >
      <PrivateMembers channel={channel} onLeft={() => onOpenChange(false)} />
    </Dialog>
  );
}

function PrivateMembers({ channel, onLeft }: { channel: Channel; onLeft: () => void }) {
  const { session } = useWorkspace();
  const loadPrivate = useCallback(() => api.privateMembers(channel.id), [channel.id]);
  const inChannel = useLive(loadPrivate, (change) => change.kind === "channels" || change.kind === "members");
  const others = useOtherMembers();
  const everyone = useLive(api.members, (change) => change.kind === "members");
  const [isAdding, setAdding] = useState(false);
  const [isLeaving, setLeaving] = useState(false);
  const [removing, setRemoving] = useState<Member | null>(null);
  if (!inChannel || !others || !everyone) return null;

  const ids = new Set(inChannel.members);
  const members = everyone.filter((member) => ids.has(member.id));
  const outsiders = others.filter((member) => !ids.has(member.id));
  const isAlone = inChannel.members.length <= 1;
  const mayLeave = !inChannel.is_owner || isAlone;

  return (
    <div className="flex flex-col gap-4">
      {isAdding ? (
        <MemberPicker
          label="People to add"
          members={outsiders}
          chosen={new Set()}
          onToggle={(member) => api.addToPrivateChannel(channel.id, member.id).catch(reportError)}
        />
      ) : (
        <MemberRows
          label="People in this channel"
          members={members}
          actions={(member) => (
            <>
              {member.id === inChannel.owner && <Badge tone="accent">Owner</Badge>}
              {inChannel.is_owner && member.id !== session.me && (
                <Menu
                  trigger={<IconButton label={`Actions for ${member.name}`} size="sm" icon={<Ellipsis size={13} />} />}
                  items={[
                    {
                      label: "Make owner",
                      icon: <Crown size={14} />,
                      onSelect: () => api.handOverPrivateChannel(channel.id, member.id).catch(reportError),
                    },
                    {
                      label: "Remove from channel",
                      icon: <UserMinus size={14} />,
                      onSelect: () => setRemoving(member),
                      isDestructive: true,
                    },
                  ]}
                />
              )}
            </>
          )}
        />
      )}
      <div className="flex items-center justify-between gap-2">
        {mayLeave ? (
          <Button variant="ghost" icon={<LogOut size={14} />} onClick={() => setLeaving(true)}>
            Leave channel
          </Button>
        ) : (
          <p className="text-[12px] text-muted">To leave, make someone else the owner first.</p>
        )}
        {inChannel.is_owner && (isAdding || outsiders.length > 0) && (
          <Button
            variant={isAdding ? "secondary" : "primary"}
            icon={isAdding ? undefined : <UserPlus size={14} />}
            onClick={() => setAdding(!isAdding)}
          >
            {isAdding ? "Done adding" : "Add people"}
          </Button>
        )}
      </div>
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(isOpen) => !isOpen && setRemoving(null)}
        title={`Remove ${removing?.name ?? ""} from #${channel.name}?`}
        description="They keep what they've already read, but can't read anything new or write here."
        confirmLabel="Remove"
        onConfirm={async () => {
          if (removing) await api.removeFromPrivateChannel(channel.id, removing.id);
        }}
      />
      <ConfirmDialog
        open={isLeaving}
        onOpenChange={setLeaving}
        title={`Leave #${channel.name}?`}
        description="You keep what you've read, but won't see anything new unless someone adds you back."
        confirmLabel="Leave channel"
        onConfirm={async () => {
          await api.leavePrivateChannel(channel.id);
          onLeft();
        }}
      />
    </div>
  );
}
