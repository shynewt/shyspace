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
  const [code, setCode] = useState<string>();
  const [copied, setCopied] = useState<string | null>(null);
  useEffect(() => {
    api.inviteCode().then(setCode, reportError);
  }, []);

  const copy = async (label: string, text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(label);
    setTimeout(() => setCopied(null), 1400);
  };

  if (!code) return null;
  const agentCommand = `shy join --bot --name "Claude" ${code}\nclaude mcp add shyspace -- shy mcp`;
  return (
    <div className="rounded-xl border border-line bg-bg p-3">
      <div className="text-[13px] font-semibold">Invite people and agents</div>
      <p className="mt-0.5 text-[12px] leading-relaxed text-muted">
        Anyone with this code can ask to join; your app lets them in while it's open. Share it privately.
      </p>
      <CopyBlock text={code} isCopied={copied === "code"} onCopy={() => copy("code", code)} />
      <div className="mt-3 flex items-center gap-1.5 text-[12px] font-medium text-muted">
        <Terminal size={13} /> Add an AI agent (Claude Code)
      </div>
      <CopyBlock text={agentCommand} isCopied={copied === "agent"} onCopy={() => copy("agent", agentCommand)} />
    </div>
  );
}

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
