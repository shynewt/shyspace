import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import { api, type Invite } from "../../lib/api";
import { reportError, showToast } from "../../lib/toast";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { Button, CopyField, Field, FormError, Input, SectionLabel } from "../../ui";

/** Makes a single-use invite code for someone, to send them privately. */
export function CreateInvite() {
  const [label, setLabel] = useState("");
  const [created, setCreated] = useState<{ code: string; label: string } | null>(null);
  const create = useFormAction(async () => {
    const code = await api.createInvite(label.trim());
    setCreated({ code, label: label.trim() });
    setLabel("");
  });
  return (
    <div className="flex flex-col gap-3">
      <form onSubmit={create.run} className="flex flex-col gap-1.5">
        <div className="flex items-end gap-2">
          <div className="flex-1">
            <Field label="Who is it for?">
              {(id) => (
                <Input id={id} value={label} onChange={(event) => setLabel(event.target.value)} placeholder="Grace" />
              )}
            </Field>
          </div>
          <Button type="submit" variant="primary" isLoading={create.isBusy} className="h-9">
            Create invite
          </Button>
        </div>
        <p className="text-xs leading-relaxed text-faint">A note for you, like their name. They don't see it.</p>
      </form>
      <FormError message={create.error} />
      <AnimatePresence>
        {created && (
          <motion.div
            key={created.code}
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            className="flex flex-col gap-1.5"
          >
            <CopyField value={created.code} label="Copy invite code" />
            <p className="text-xs leading-relaxed text-muted">
              Send this code to {created.label || "them"} privately. It works once, within 7 days, and holds no
              passwords. When it's used, you approve the request under Requests.
            </p>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

/** Invite codes nobody has used yet, each with a way to take it back. */
export function OpenInvites() {
  const invites = useLive(api.invites, (change) => change.kind === "invites" || change.kind === "members");
  const open = invites?.filter((invite) => invite.state === "open") ?? [];
  return (
    <div>
      <SectionLabel>Codes waiting to be used</SectionLabel>
      {open.length === 0 ? (
        <p className="px-2.5 text-sm text-muted">None right now.</p>
      ) : (
        <ul aria-label="Codes waiting to be used" className="flex flex-col">
          {open.map((invite) => (
            <OpenInvite key={invite.token} invite={invite} />
          ))}
        </ul>
      )}
    </div>
  );
}

function OpenInvite({ invite }: { invite: Invite }) {
  const name = invite.label || "Unlabeled invite";
  const revoke = () =>
    api.revokeInvite(invite.token).then(() => showToast("Invite revoked. The code no longer works."), reportError);
  return (
    <li className="flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-sm">
      <span className="min-w-0 flex-1 truncate">{name}</span>
      <span className="shrink-0 text-xs text-muted">
        Expires {new Date(invite.expires_ms).toLocaleDateString([], { month: "short", day: "numeric" })}
      </span>
      <Button
        variant="ghost"
        size="sm"
        aria-label={invite.label ? `Revoke the invite for ${invite.label}` : "Revoke this unlabeled invite"}
        onClick={revoke}
      >
        Revoke
      </Button>
    </li>
  );
}
