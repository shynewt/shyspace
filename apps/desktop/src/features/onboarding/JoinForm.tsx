import { KeyRound, ShieldCheck, Ticket } from "lucide-react";
import { motion } from "motion/react";
import { type FormEvent, useRef, useState } from "react";
import { api, type Session } from "../../lib/api";
import { MAX_PROFILE_NAME_CHARS } from "../../lib/limits";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Button, Field, Input, TextArea } from "../../ui";
import { SubmitButton } from "./SubmitButton";

export function JoinForm({ onReady }: { onReady: (session: Session) => void }) {
  const [invite, setInvite] = useState("");
  const [name, setName] = useState("");
  const [safetyCode, setSafetyCode] = useState<string | null>(null);
  const [isAsking, setAsking] = useState(false);
  const isCancelling = useRef(false);
  const firstInput = useRef<HTMLTextAreaElement>(null);
  useFocusOnMount(firstInput);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (isAsking) return;
    isCancelling.current = false;
    setAsking(true);
    try {
      setSafetyCode(await api.safetyCode());
      onReady(await api.joinWorkspace(invite, name));
    } catch (error) {
      // A join you cancelled ends in an error too; it needs no message.
      if (!isCancelling.current) reportError(error);
      setSafetyCode(null);
      setAsking(false);
    }
  };
  const cancel = () => {
    isCancelling.current = true;
    api.cancelJoin().catch((error: unknown) => {
      isCancelling.current = false;
      reportError(error);
    });
  };

  if (safetyCode) return <WaitingForApproval safetyCode={safetyCode} onCancel={cancel} />;

  return (
    <form onSubmit={submit} className="flex flex-col gap-3">
      <Field label="Invite code">
        {(id) => (
          <TextArea
            id={id}
            ref={firstInput}
            value={invite}
            onChange={(event) => setInvite(event.target.value)}
            placeholder="shy2_…"
            rows={4}
            className="font-mono text-xs"
          />
        )}
      </Field>
      <Field label="Your name">
        {(id) => (
          <Input
            id={id}
            value={name}
            maxLength={MAX_PROFILE_NAME_CHARS}
            onChange={(event) => setName(event.target.value)}
            placeholder="Grace Hopper"
          />
        )}
      </Field>
      <SubmitButton
        isBusy={isAsking}
        isDisabled={!invite.trim().startsWith("shy2_") || !name.trim()}
        icon={<Ticket size={15} />}
      >
        Ask to join
      </SubmitButton>
    </form>
  );
}

function WaitingForApproval({ safetyCode, onCancel }: { safetyCode: string; onCancel: () => void }) {
  return (
    <div className="flex flex-col items-center py-4 text-center">
      <div className="relative grid h-24 w-24 place-items-center">
        {[0, 1, 2].map((ring) => (
          <motion.span
            key={ring}
            className="absolute inset-0 rounded-full border-2 border-accent/40"
            initial={{ scale: 0.4, opacity: 0.9 }}
            animate={{ scale: 1.25, opacity: 0 }}
            transition={{ duration: 2.4, repeat: Number.POSITIVE_INFINITY, delay: ring * 0.8, ease: "easeOut" }}
          />
        ))}
        <div className="grid h-12 w-12 place-items-center rounded-2xl bg-accent-soft text-accent-text">
          <KeyRound size={22} />
        </div>
      </div>
      <h2 className="mt-4 text-lg font-semibold">Waiting for the admin…</h2>
      <p className="mt-1 max-w-[340px] text-sm leading-relaxed text-muted">
        They approve your request from their app. Read them your safety code so they know it's really you.
      </p>
      <div className="mt-3 rounded-xl border border-line bg-bg px-4 py-3">
        <div className="flex items-center justify-center gap-1.5 text-xs font-medium text-muted">
          <ShieldCheck size={13} /> Your safety code
        </div>
        <div className="selectable mt-1 font-mono text-lg tracking-wider">{safetyCode}</div>
      </div>
      <p className="mt-3 text-xs text-faint">Keep this window open.</p>
      <Button variant="ghost" onClick={onCancel} className="mt-2">
        Cancel request
      </Button>
    </div>
  );
}
