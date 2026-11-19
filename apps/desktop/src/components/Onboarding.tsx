import { useState, type FormEvent, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ArrowRight, KeyRound, Lock, Sparkles, Ticket } from "lucide-react";
import { api, type BucketConfig, type Session } from "../lib/api";
import { reportError } from "../lib/toast";
import { Logo } from "./Logo";

type Mode = "create" | "join";

const EMPTY_BUCKET: BucketConfig = {
  endpoint: "",
  region: "us-east-1",
  bucket: "",
  access_key_id: "",
  secret_access_key: "",
};

export function Onboarding({ onReady }: { onReady: (session: Session) => void }) {
  const [mode, setMode] = useState<Mode>("create");
  return (
    <motion.main
      className="relative grid h-full place-items-center overflow-auto px-6 py-10"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, transition: { duration: 0.2 } }}
    >
      <Backdrop />
      <div className="relative w-full max-w-[460px]">
        <motion.div
          className="mb-7 flex flex-col items-center text-center"
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
        >
          <Logo size={48} />
          <h1 className="mt-4 text-[26px] font-semibold tracking-tight">Welcome to shyspace</h1>
          <p className="mt-1.5 text-muted">Chat, pages and agents, end-to-end encrypted. No server to run.</p>
        </motion.div>

        <motion.div
          className="rounded-2xl border border-line bg-surface p-2 shadow-float"
          initial={{ opacity: 0, y: 18, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          transition={{ delay: 0.05 }}
        >
          <div className="relative grid grid-cols-2 rounded-xl bg-surface-2 p-1">
            {(["create", "join"] as const).map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => setMode(option)}
                className={`relative z-10 rounded-lg py-2 text-[13px] font-medium transition-colors ${
                  mode === option ? "text-ink" : "text-muted hover:text-ink"
                }`}
              >
                {mode === option && (
                  <motion.span layoutId="mode-pill" className="absolute inset-0 -z-10 rounded-lg bg-surface shadow-card" />
                )}
                {option === "create" ? "Create a workspace" : "Join with an invite"}
              </button>
            ))}
          </div>
          <div className="px-4 pt-5 pb-4">
            <AnimatePresence mode="wait" initial={false}>
              {mode === "create" ? (
                <Swap key="create">
                  <CreateForm onReady={onReady} />
                </Swap>
              ) : (
                <Swap key="join">
                  <JoinForm onReady={onReady} />
                </Swap>
              )}
            </AnimatePresence>
          </div>
        </motion.div>
      </div>
    </motion.main>
  );
}

function Swap({ children }: { children: ReactNode }) {
  return (
    <motion.div
      initial={{ opacity: 0, x: 12 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -12 }}
      transition={{ duration: 0.18, ease: "easeOut" }}
    >
      {children}
    </motion.div>
  );
}

function CreateForm({ onReady }: { onReady: (session: Session) => void }) {
  const [workspaceName, setWorkspaceName] = useState("");
  const [name, setName] = useState("");
  const [bucket, setBucket] = useState(EMPTY_BUCKET);
  const [busy, setBusy] = useState(false);
  const setField = (field: keyof BucketConfig) => (value: string) => setBucket({ ...bucket, [field]: value });
  const isComplete = workspaceName.trim() && name.trim() && Object.values(bucket).every((value) => value.trim());

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      onReady(await api.createWorkspace(workspaceName, name, bucket));
    } catch (error) {
      reportError(error);
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-3">
        <Field label="Workspace" value={workspaceName} onChange={setWorkspaceName} placeholder="Acme" autoFocus />
        <Field label="Your name" value={name} onChange={setName} placeholder="Ada Lovelace" />
      </div>
      <div className="mt-2 flex items-center gap-2 text-[12px] font-medium tracking-wide text-muted uppercase">
        <Lock size={13} /> Storage bucket
      </div>
      <p className="-mt-1 text-[12.5px] leading-relaxed text-muted">
        Any S3-compatible bucket (AWS, Cloudflare R2, MinIO…). It only ever stores encrypted data and is used as a
        mailbox when teammates are offline.
      </p>
      <Field label="Endpoint" value={bucket.endpoint} onChange={setField("endpoint")} placeholder="https://s3.eu-west-1.amazonaws.com" />
      <div className="grid grid-cols-2 gap-3">
        <Field label="Bucket" value={bucket.bucket} onChange={setField("bucket")} placeholder="team-space" />
        <Field label="Region" value={bucket.region} onChange={setField("region")} />
      </div>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Access key" value={bucket.access_key_id} onChange={setField("access_key_id")} />
        <Field label="Secret key" value={bucket.secret_access_key} onChange={setField("secret_access_key")} type="password" />
      </div>
      <SubmitButton busy={busy} disabled={!isComplete} icon={<Sparkles size={15} />}>
        {busy ? "Creating your workspace…" : "Create workspace"}
      </SubmitButton>
    </form>
  );
}

function JoinForm({ onReady }: { onReady: (session: Session) => void }) {
  const [invite, setInvite] = useState("");
  const [name, setName] = useState("");
  const [waiting, setWaiting] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setWaiting(true);
    try {
      onReady(await api.joinWorkspace(invite, name));
    } catch (error) {
      reportError(error);
      setWaiting(false);
    }
  };

  if (waiting) return <WaitingForAdmin />;

  return (
    <form onSubmit={submit} className="flex flex-col gap-3">
      <label className="flex flex-col gap-1.5">
        <span className="text-[12px] font-medium text-muted">Invite code</span>
        <textarea
          value={invite}
          onChange={(event) => setInvite(event.target.value)}
          placeholder="shy1_…"
          rows={4}
          autoFocus
          className="resize-none rounded-lg border border-line bg-bg px-3 py-2 font-mono text-[12px] leading-relaxed outline-none transition focus:border-accent focus:ring-3 focus:ring-accent/15"
        />
      </label>
      <Field label="Your name" value={name} onChange={setName} placeholder="Grace Hopper" />
      <SubmitButton busy={false} disabled={!invite.trim().startsWith("shy1_") || !name.trim()} icon={<Ticket size={15} />}>
        Join workspace
      </SubmitButton>
    </form>
  );
}

function WaitingForAdmin() {
  return (
    <div className="flex flex-col items-center py-6 text-center">
      <div className="relative grid h-24 w-24 place-items-center">
        {[0, 1, 2].map((ring) => (
          <motion.span
            key={ring}
            className="absolute inset-0 rounded-full border-2 border-accent/40"
            initial={{ scale: 0.4, opacity: 0.9 }}
            animate={{ scale: 1.25, opacity: 0 }}
            transition={{ duration: 2.4, repeat: Infinity, delay: ring * 0.8, ease: "easeOut" }}
          />
        ))}
        <div className="grid h-12 w-12 place-items-center rounded-2xl bg-accent-soft text-accent-text">
          <KeyRound size={22} />
        </div>
      </div>
      <h2 className="mt-4 text-[15px] font-semibold">Asking to join…</h2>
      <p className="mt-1 max-w-[320px] text-[13px] leading-relaxed text-muted">
        The admin's app lets you in automatically the next time it's online. Your keys are being exchanged securely.
      </p>
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  placeholder,
  type = "text",
  autoFocus = false,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: string;
  autoFocus?: boolean;
}) {
  return (
    <label className="flex min-w-0 flex-col gap-1.5">
      <span className="text-[12px] font-medium text-muted">{label}</span>
      <input
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        autoFocus={autoFocus}
        spellCheck={false}
        className="h-9 rounded-lg border border-line bg-bg px-3 text-[13px] outline-none transition placeholder:text-faint focus:border-accent focus:ring-3 focus:ring-accent/15"
      />
    </label>
  );
}

function SubmitButton({
  busy,
  disabled,
  icon,
  children,
}: {
  busy: boolean;
  disabled: boolean;
  icon: ReactNode;
  children: ReactNode;
}) {
  return (
    <motion.button
      type="submit"
      disabled={disabled || busy}
      whileHover={disabled || busy ? undefined : { y: -1 }}
      whileTap={disabled || busy ? undefined : { scale: 0.98 }}
      className="group mt-2 flex h-10 items-center justify-center gap-2 rounded-lg bg-accent font-medium text-white shadow-card transition-colors hover:bg-accent-strong disabled:cursor-not-allowed disabled:opacity-45"
    >
      {busy ? (
        <motion.span
          className="h-4 w-4 rounded-full border-2 border-white/40 border-t-white"
          animate={{ rotate: 360 }}
          transition={{ duration: 0.8, repeat: Infinity, ease: "linear" }}
        />
      ) : (
        icon
      )}
      {children}
      {!busy && <ArrowRight size={15} className="transition-transform group-hover:translate-x-0.5" />}
    </motion.button>
  );
}

function Backdrop() {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
      <motion.div
        className="absolute -top-40 left-1/2 h-[520px] w-[820px] -translate-x-1/2 rounded-full opacity-60 blur-3xl"
        style={{ background: "radial-gradient(closest-side, var(--accent-soft), transparent)" }}
        animate={{ scale: [1, 1.08, 1] }}
        transition={{ duration: 9, repeat: Infinity, ease: "easeInOut" }}
      />
    </div>
  );
}
