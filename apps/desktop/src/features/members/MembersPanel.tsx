import { Bot, Crown, UserMinus } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import { api, type Member } from "../../lib/api";
import { displayName } from "../../lib/people";
import { reportError, showToast } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, Button, Drawer, SectionLabel, Tooltip } from "../../ui";
import { Invites } from "./Invites";
import { Recovery } from "./Recovery";

export function MembersPanel({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const { session } = useWorkspace();
  return (
    <Drawer open={open} onOpenChange={onOpenChange} title="Members">
      {session.is_admin && <Invites />}
      <MemberList />
      {session.is_admin && <Recovery />}
    </Drawer>
  );
}

function MemberList() {
  const { session } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members") ?? [];
  const active = members.filter((member) => member.active);
  const former = members.filter((member) => !member.active);
  return (
    <>
      <div className="mt-3">
        <SectionLabel>Members · {active.length}</SectionLabel>
        <ul className="flex flex-col">
          <AnimatePresence initial={false}>
            {active.map((member) => (
              <MemberRow key={member.id} member={member} isMe={member.id === session.me} canRemove={session.is_admin} />
            ))}
          </AnimatePresence>
        </ul>
      </div>
      {former.length > 0 && (
        <div className="mt-3 opacity-60">
          <SectionLabel>Former members</SectionLabel>
          <ul>
            {former.map((member) => (
              <MemberRow key={member.id} member={member} isMe={false} canRemove={false} />
            ))}
          </ul>
        </div>
      )}
    </>
  );
}

function MemberRow({ member, isMe, canRemove }: { member: Member; isMe: boolean; canRemove: boolean }) {
  const [isConfirming, setConfirming] = useState(false);
  const [isRemoving, setRemoving] = useState(false);
  const name = displayName(member.name, member.id);
  const remove = async () => {
    setRemoving(true);
    try {
      await api.removeMember(member.id);
      showToast(`${name} was removed. New content uses fresh keys they don't have.`);
    } catch (error) {
      reportError(error);
      setRemoving(false);
    }
  };
  return (
    <motion.li
      layout
      initial={{ opacity: 0, x: 12 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -12, height: 0 }}
      className="group flex items-center gap-3 rounded-lg px-2 py-2 hover:bg-surface-2/60"
      onMouseLeave={() => setConfirming(false)}
    >
      <Avatar id={member.id} name={name} isBot={member.is_bot} size={32} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 text-[13.5px] font-medium">
          <span className="truncate">{name}</span>
          {isMe && <span className="text-[12px] font-normal text-faint">(you)</span>}
          {member.is_admin && (
            <Tooltip label="Admin">
              <Crown size={13} className="shrink-0 text-warning" aria-label="Admin" />
            </Tooltip>
          )}
          {member.is_bot && (
            <Tooltip label="Bot">
              <Bot size={13} className="shrink-0 text-accent" aria-label="Bot" />
            </Tooltip>
          )}
        </div>
        <div className="truncate font-mono text-[11px] text-faint">{member.id.slice(0, 16)}…</div>
      </div>
      {canRemove && !member.is_admin && member.active && (
        <Button
          size="sm"
          variant={isConfirming ? "danger" : "ghost"}
          icon={<UserMinus size={13} />}
          isLoading={isRemoving}
          onClick={() => (isConfirming ? remove() : setConfirming(true))}
          className={isConfirming ? "" : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100"}
        >
          {isConfirming ? "Confirm" : "Remove"}
        </Button>
      )}
    </motion.li>
  );
}
