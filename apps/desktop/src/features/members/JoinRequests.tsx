import { Bot, ShieldCheck } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import { api, type Invite } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { Button, Card, springs } from "../../ui";

/** Join requests waiting for a decision. Only the admin may load them. */
export function useJoinRequests(): Invite[] | undefined {
  const invites = useLive(api.invites, (change) => change.kind === "invites" || change.kind === "members");
  return invites?.filter((invite) => invite.state === "requested");
}

/** How many join requests wait: a count that needs the admin. */
export function JoinRequestsBadge({ count }: { count: number }) {
  return (
    <AnimatePresence>
      {count > 0 && (
        <motion.span
          key={count}
          initial={{ scale: 0.4, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          exit={{ scale: 0.4, opacity: 0 }}
          transition={springs.pop}
          aria-label={`${count} join ${count === 1 ? "request" : "requests"} waiting`}
          className="relative ml-auto grid h-[18px] min-w-[18px] shrink-0 place-items-center rounded-full bg-accent px-1.5 text-2xs font-semibold text-on-accent tabular-nums"
        >
          {count}
        </motion.span>
      )}
    </AnimatePresence>
  );
}

/** The badge for wherever the requests themselves aren't loaded yet. Admin only. */
export function WaitingRequestsBadge() {
  return <JoinRequestsBadge count={useJoinRequests()?.length ?? 0} />;
}

export function JoinRequestList() {
  const requests = useJoinRequests();
  if (!requests) return null;
  if (requests.length === 0) {
    return (
      <p className="px-2 py-6 text-center text-sm text-muted">
        No one is waiting to join. Requests show here when someone uses an invite.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      <AnimatePresence initial={false}>
        {requests.map((invite) => (
          <JoinRequest key={invite.token} invite={invite} />
        ))}
      </AnimatePresence>
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
        <div className="flex items-center gap-1.5 text-sm font-semibold">
          {invite.requester_is_bot && <Bot size={14} className="text-accent" />}
          {invite.requester_name} wants to join
        </div>
        {invite.label && <div className="mt-0.5 text-xs text-muted">Invite: {invite.label}</div>}
        {invite.requester_is_bot && (
          <p className="mt-1.5 text-xs leading-relaxed text-muted">
            This is an AI agent. It will read every channel and page, and what it reads goes to its AI provider:
            end-to-end encryption ends there.
          </p>
        )}
        <div className="mt-2 rounded-lg bg-surface px-2.5 py-2">
          <div className="flex items-center gap-1.5 text-xs font-medium text-muted">
            <ShieldCheck size={13} /> Safety code
          </div>
          <div className="selectable mt-0.5 font-mono text-sm tracking-wider">{invite.requester_safety_code}</div>
          <p className="mt-1 text-xs leading-relaxed text-muted">
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
