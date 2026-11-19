import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Bot, Check, Copy, Crown, Terminal, UserMinus, X } from "lucide-react";
import { api, type Member } from "../lib/api";
import { useLive } from "../lib/useLive";
import { displayName } from "../lib/people";
import { reportError, showToast } from "../lib/toast";
import { useWorkspace } from "../lib/workspace";
import { Avatar } from "./Avatar";

export function MembersPanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            className="fixed inset-0 z-30 bg-black/20"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={onClose}
          />
          <motion.aside
            className="fixed inset-y-0 right-0 z-30 flex w-[380px] max-w-[92vw] flex-col border-l border-line bg-surface shadow-float"
            initial={{ x: "100%" }}
            animate={{ x: 0 }}
            exit={{ x: "100%", transition: { duration: 0.18, ease: "easeIn" } }}
            transition={{ type: "spring", stiffness: 420, damping: 40 }}
          >
            <PanelBody onClose={onClose} />
          </motion.aside>
        </>
      )}
    </AnimatePresence>
  );
}

function PanelBody({ onClose }: { onClose: () => void }) {
  const { session } = useWorkspace();
  const members = useLive(api.members, [], (change) => change.kind === "members") ?? [];
  const active = members.filter((member) => member.active);
  const former = members.filter((member) => !member.active);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <>
      <header className="flex h-13 shrink-0 items-center justify-between border-b border-line px-5">
        <h2 className="font-semibold">Members</h2>
        <button onClick={onClose} aria-label="Close" className="rounded-md p-1 text-muted hover:bg-surface-2 hover:text-ink">
          <X size={16} />
        </button>
      </header>
      <div className="flex-1 overflow-y-auto p-3">
        {session.is_admin && <InviteCard />}
        <ul className="mt-2 flex flex-col">
          <AnimatePresence initial={false}>
            {active.map((member) => (
              <MemberRow key={member.id} member={member} isMe={member.id === session.me} canRemove={session.is_admin} />
            ))}
          </AnimatePresence>
        </ul>
        {former.length > 0 && (
          <>
            <div className="px-2 pt-4 pb-1 text-[11px] font-semibold tracking-wide text-faint uppercase">Former members</div>
            <ul className="opacity-60">
              {former.map((member) => (
                <MemberRow key={member.id} member={member} isMe={false} canRemove={false} />
              ))}
            </ul>
          </>
        )}
      </div>
    </>
  );
}

function MemberRow({ member, isMe, canRemove }: { member: Member; isMe: boolean; canRemove: boolean }) {
  const [isConfirming, setConfirming] = useState(false);
  const name = displayName(member.name, member.id);
  const remove = async () => {
    try {
      await api.removeMember(member.id);
      showToast(`${name} was removed. New content uses fresh keys they don't have.`);
    } catch (error) {
      reportError(error);
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
          {member.is_admin && <Crown size={13} className="shrink-0 text-warning" aria-label="Admin" />}
          {member.is_bot && <Bot size={13} className="shrink-0 text-accent" aria-label="Bot" />}
        </div>
        <div className="truncate font-mono text-[11px] text-faint">{member.id.slice(0, 16)}…</div>
      </div>
      {canRemove && !member.is_admin && member.active && (
        <motion.button
          layout
          onClick={() => (isConfirming ? remove() : setConfirming(true))}
          className={`flex h-7 items-center gap-1 rounded-md px-2 text-[12px] font-medium opacity-0 transition group-hover:opacity-100 ${
            isConfirming ? "bg-danger text-white opacity-100" : "text-muted hover:bg-danger/10 hover:text-danger"
          }`}
        >
          <UserMinus size={13} />
          {isConfirming ? "Confirm" : "Remove"}
        </motion.button>
      )}
    </motion.li>
  );
}

function InviteCard() {
  const invites = useLive(api.invites, [], (change) => change.kind === "invites" || change.kind === "members") ?? [];
  const [label, setLabel] = useState("");
  const [newCode, setNewCode] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const requests = invites.filter((invite) => invite.state === "requested");
  const open = invites.filter((invite) => invite.state === "open");

  const copy = async (key: string, text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(key);
    setTimeout(() => setCopied(null), 1400);
  };
  const create = async () => {
    try {
      setNewCode(await api.createInvite(label));
      setLabel("");
    } catch (error) {
      reportError(error);
    }
  };
  const decide = (action: (token: string) => Promise<void>, token: string) => action(token).catch(reportError);

  return (
    <div className="flex flex-col gap-3">
      {requests.map((invite) => (
        <motion.div key={invite.token} layout className="rounded-xl border border-accent/40 bg-accent-soft/40 p-3">
          <div className="text-[13px] font-semibold">
            {invite.requester_name} wants to join{invite.requester_is_bot ? " (bot)" : ""}
          </div>
          <div className="mt-0.5 font-mono text-[11px] text-muted">key {invite.requester_id?.slice(0, 16)}…</div>
          {invite.label && <div className="mt-0.5 text-[12px] text-muted">Invite: {invite.label}</div>}
          <div className="mt-2 flex gap-2">
            <button onClick={() => decide(api.approveInvite, invite.token)} className="h-7 rounded-md bg-accent px-3 text-[12px] font-medium text-white">
              Let in
            </button>
            <button onClick={() => decide(api.rejectInvite, invite.token)} className="h-7 rounded-md px-3 text-[12px] text-muted hover:bg-surface-2">
              Decline
            </button>
          </div>
        </motion.div>
      ))}
      <div className="rounded-xl border border-line bg-bg p-3">
        <div className="text-[13px] font-semibold">Invite someone</div>
        <p className="mt-0.5 text-[12px] leading-relaxed text-muted">
          Each code works once, for 7 days, and you approve the request. It contains no passwords.
        </p>
        <div className="mt-2 flex gap-2">
          <input
            value={label}
            onChange={(event) => setLabel(event.target.value)}
            placeholder="Who is it for? (optional)"
            className="h-8 min-w-0 flex-1 rounded-lg border border-line bg-surface px-2.5 text-[12.5px] outline-none focus:border-accent"
          />
          <button onClick={create} className="h-8 rounded-lg bg-accent px-3 text-[12.5px] font-medium text-white">
            Create
          </button>
        </div>
        {newCode && <CopyBlock text={newCode} isCopied={copied === "code"} onCopy={() => copy("code", newCode)} />}
        <div className="mt-3 flex items-center gap-1.5 text-[12px] font-medium text-muted">
          <Terminal size={13} /> Add an AI agent (Claude Code)
        </div>
        <CopyBlock
          text={AGENT_COMMAND}
          isCopied={copied === "agent"}
          onCopy={() => copy("agent", AGENT_COMMAND)}
        />
      </div>
      {open.length > 0 && (
        <div className="flex flex-col gap-1">
          <div className="px-1 text-[11px] font-semibold tracking-wide text-faint uppercase">Waiting to be used</div>
          {open.map((invite) => (
            <div key={invite.token} className="flex items-center gap-2 rounded-lg px-2 py-1.5 text-[12.5px] hover:bg-surface-2/60">
              <span className="min-w-0 flex-1 truncate">{invite.label || "Unlabeled invite"}</span>
              <span className="text-[11px] text-faint">expires {new Date(invite.expires_ms).toLocaleDateString()}</span>
              <button onClick={() => decide(api.revokeInvite, invite.token)} className="rounded px-1.5 text-[11.5px] text-muted hover:text-danger">
                Revoke
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

const AGENT_COMMAND = `shy join --bot --name "Claude" <invite code>\nclaude mcp add shyspace -- shy mcp`;

function CopyBlock({ text, isCopied, onCopy }: { text: string; isCopied: boolean; onCopy: () => void }) {
  return (
    <div className="group relative mt-2">
      <pre className="selectable max-h-24 overflow-auto rounded-lg border border-line bg-surface p-2 pr-9 font-mono text-[11px] leading-relaxed break-all whitespace-pre-wrap text-muted">
        {text}
      </pre>
      <button onClick={onCopy} aria-label="Copy" className="absolute top-1.5 right-1.5 grid h-6 w-6 place-items-center rounded-md bg-surface-2 text-muted hover:text-ink">
        <AnimatePresence mode="wait" initial={false}>
          <motion.span key={isCopied ? "done" : "copy"} initial={{ scale: 0.5, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} exit={{ scale: 0.5, opacity: 0 }}>
            {isCopied ? <Check size={13} className="text-success" /> : <Copy size={13} />}
          </motion.span>
        </AnimatePresence>
      </button>
    </div>
  );
}
