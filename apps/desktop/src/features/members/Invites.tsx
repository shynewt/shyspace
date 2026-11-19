import { Bot, ShieldCheck, Terminal } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type FormEvent, useState } from "react";
import { api, type Invite } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { Button, Card, CopyField, Input, SectionLabel, springs } from "../../ui";

const AGENT_COMMAND = `shy join --bot --name "Claude" <invite code>\nclaude mcp add shyspace -- shy mcp`;

export function Invites() {
  const invites = useLive(api.invites, (change) => change.kind === "invites" || change.kind === "members") ?? [];
  const requests = invites.filter((invite) => invite.state === "requested");
  const waiting = invites.filter((invite) => invite.state === "open");
  return (
    <div className="flex flex-col gap-3">
      <AnimatePresence initial={false}>
        {requests.map((invite) => (
          <JoinRequest key={invite.token} invite={invite} />
        ))}
      </AnimatePresence>
      <CreateInvite />
      {waiting.length > 0 && (
        <div>
          <SectionLabel>Waiting to be used</SectionLabel>
          {waiting.map((invite) => (
            <OpenInvite key={invite.token} invite={invite} />
          ))}
        </div>
      )}
    </div>
  );
}

function JoinRequest({ invite }: { invite: Invite }) {
  const [busyAction, setBusyAction] = useState<"approve" | "reject" | null>(null);
  const decide = async (action: "approve" | "reject") => {
    setBusyAction(action);
    try {
      await (action === "approve" ? api.approveInvite(invite.token) : api.rejectInvite(invite.token));
    } catch (error) {
      reportError(error);
      setBusyAction(null);
    }
  };
  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0 }}
      transition={springs.smooth}
    >
      <Card highlighted>
        <div className="flex items-center gap-1.5 text-[13px] font-semibold">
          {invite.requester_is_bot && <Bot size={14} className="text-accent" />}
          {invite.requester_name} wants to join
        </div>
        {invite.label && <div className="mt-0.5 text-[12px] text-muted">Invite: {invite.label}</div>}
        {invite.requester_is_bot && (
          <p className="mt-1.5 text-[12px] leading-relaxed text-muted">
            This is an AI agent. It will read every channel and page, and what it reads goes to its AI provider:
            end-to-end encryption ends there.
          </p>
        )}
        <div className="mt-2 rounded-lg bg-surface px-2.5 py-2">
          <div className="flex items-center gap-1.5 text-[11.5px] font-medium text-muted">
            <ShieldCheck size={13} /> Safety code
          </div>
          <div className="selectable mt-0.5 font-mono text-[13px] tracking-wider">{invite.requester_safety_code}</div>
          <p className="mt-1 text-[11.5px] leading-relaxed text-faint">
            Ask them to read you the code their screen shows. Only let them in if it matches.
          </p>
        </div>
        <div className="mt-2.5 flex gap-2">
          <Button variant="primary" size="sm" isLoading={busyAction === "approve"} onClick={() => decide("approve")}>
            Codes match, let in
          </Button>
          <Button variant="ghost" size="sm" isLoading={busyAction === "reject"} onClick={() => decide("reject")}>
            Decline
          </Button>
        </div>
      </Card>
    </motion.div>
  );
}

function CreateInvite() {
  const [label, setLabel] = useState("");
  const [code, setCode] = useState<string | null>(null);
  const [isCreating, setCreating] = useState(false);
  const create = async (event: FormEvent) => {
    event.preventDefault();
    setCreating(true);
    try {
      setCode(await api.createInvite(label));
      setLabel("");
    } catch (error) {
      reportError(error);
    } finally {
      setCreating(false);
    }
  };
  return (
    <Card>
      <div className="text-[13px] font-semibold">Invite someone</div>
      <p className="mt-0.5 text-[12px] leading-relaxed text-muted">
        Each code works once, for 7 days, and you approve the request. It contains no passwords.
      </p>
      <form onSubmit={create} className="mt-2 flex gap-2">
        <Input
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          placeholder="Who is it for? (optional)"
          aria-label="Who the invite is for"
          className="h-8"
        />
        <Button type="submit" variant="primary" isLoading={isCreating}>
          Create
        </Button>
      </form>
      <AnimatePresence>
        {code && (
          <motion.div initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: "auto" }} className="mt-2">
            <CopyField value={code} label="Copy invite code" />
          </motion.div>
        )}
      </AnimatePresence>
      <div className="mt-3 flex items-center gap-1.5 text-[12px] font-medium text-muted">
        <Terminal size={13} /> Add an AI agent (Claude Code)
      </div>
      <div className="mt-1.5">
        <CopyField value={AGENT_COMMAND} label="Copy commands" />
      </div>
    </Card>
  );
}

function OpenInvite({ invite }: { invite: Invite }) {
  return (
    <div className="flex items-center gap-2 rounded-lg px-2 py-1.5 text-[12.5px] hover:bg-surface-2/60">
      <span className="min-w-0 flex-1 truncate">{invite.label || "Unlabeled invite"}</span>
      <span className="text-[11px] text-faint">expires {new Date(invite.expires_ms).toLocaleDateString()}</span>
      <Button variant="ghost" size="sm" onClick={() => api.revokeInvite(invite.token).catch(reportError)}>
        Revoke
      </Button>
    </div>
  );
}
