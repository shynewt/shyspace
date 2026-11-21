import {
  ArrowRight,
  CircleAlert,
  CircleCheck,
  KeyRound,
  LifeBuoy,
  Lock,
  PlugZap,
  ShieldCheck,
  Sparkles,
  Ticket,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type FormEvent, type ReactNode, useRef, useState } from "react";
import { api, type BucketConfig, type Session } from "../../lib/api";
import { MAX_PROFILE_NAME_CHARS } from "../../lib/limits";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Button, Field, Input, Logo, rise, SegmentedControl, TextArea } from "../../ui";

type Mode = "create" | "join" | "restore";

const MODES: Array<{ value: Mode; label: string }> = [
  { value: "create", label: "Create" },
  { value: "join", label: "Join" },
  { value: "restore", label: "Restore admin" },
];

const EMPTY_BUCKET: BucketConfig = {
  endpoint: "",
  region: "us-east-1",
  bucket: "",
  access_key_id: "",
  secret_access_key: "",
};

/** The local test bucket from docs/development.md, filled in for debug builds only. */
const LOCAL_BUCKET: BucketConfig = {
  endpoint: "http://localhost:9000",
  region: "us-east-1",
  bucket: "shyspace",
  access_key_id: "shyadmin",
  secret_access_key: "shysecret123",
};

const STARTING_BUCKET = import.meta.env.TAURI_ENV_DEBUG === "true" ? LOCAL_BUCKET : EMPTY_BUCKET;

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
          <h1 className="mt-4 text-2xl font-semibold tracking-tight">Welcome to shyspace</h1>
          <p className="mt-1.5 text-muted">Chat, pages and agents, end-to-end encrypted. No server to run.</p>
        </motion.div>

        <motion.div
          className="rounded-2xl border border-line bg-surface p-2 shadow-float"
          initial={{ opacity: 0, y: 18, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          transition={{ delay: 0.05 }}
        >
          <SegmentedControl label="How to start" options={MODES} value={mode} onChange={setMode} width="full" />
          <div className="px-4 pt-5 pb-4">
            <AnimatePresence mode="wait" initial={false}>
              <motion.div
                key={mode}
                initial={{ opacity: 0, x: 12 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: -12 }}
                transition={{ duration: 0.18, ease: "easeOut" }}
              >
                {mode === "create" && <CreateForm onReady={onReady} />}
                {mode === "join" && <JoinForm onReady={onReady} />}
                {mode === "restore" && <RestoreForm onReady={onReady} />}
              </motion.div>
            </AnimatePresence>
          </div>
        </motion.div>
      </div>
    </motion.main>
  );
}

function CreateForm({ onReady }: { onReady: (session: Session) => void }) {
  const [workspaceName, setWorkspaceName] = useState("");
  const [name, setName] = useState("");
  const [bucket, setBucket] = useState(STARTING_BUCKET);
  const [isBusy, setBusy] = useState(false);
  const firstInput = useRef<HTMLInputElement>(null);
  useFocusOnMount(firstInput);
  const setField = (field: keyof BucketConfig) => (value: string) => setBucket({ ...bucket, [field]: value });
  const isComplete = workspaceName.trim() && name.trim() && Object.values(bucket).every((value) => value.trim());
  const [check, setCheck] = useState<BucketCheck>({ kind: "untested" });
  // A passed check only counts for the exact settings that were tested.
  const settingsKey = JSON.stringify(bucket);
  const isBucketVerified = check.kind === "passed" && check.settingsKey === settingsKey;

  const testBucket = async () => {
    setCheck({ kind: "testing" });
    try {
      await api.testBucket(bucket);
      setCheck({ kind: "passed", settingsKey });
    } catch (error) {
      setCheck({ kind: "failed", message: String(error) });
    }
  };

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
        <Field label="Workspace">
          {(id) => (
            <Input
              id={id}
              ref={firstInput}
              value={workspaceName}
              onChange={(event) => setWorkspaceName(event.target.value)}
              placeholder="Acme"
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
              placeholder="Ada Lovelace"
            />
          )}
        </Field>
      </div>
      <div className="mt-2 flex items-center gap-2 text-xs font-medium tracking-wide text-muted uppercase">
        <Lock size={13} /> Storage bucket
      </div>
      <p className="-mt-1 text-sm leading-relaxed text-muted">
        Any S3-compatible bucket (AWS, Cloudflare R2, MinIO…). It only ever stores encrypted data and is the mailbox
        while teammates are offline.
      </p>
      <BucketField
        label="Endpoint"
        value={bucket.endpoint}
        onChange={setField("endpoint")}
        placeholder="https://s3.eu-west-1.amazonaws.com"
      />
      <div className="grid grid-cols-2 gap-3">
        <BucketField label="Bucket" value={bucket.bucket} onChange={setField("bucket")} placeholder="team-space" />
        <BucketField label="Region" value={bucket.region} onChange={setField("region")} />
      </div>
      <div className="grid grid-cols-2 gap-3">
        <BucketField label="Access key" value={bucket.access_key_id} onChange={setField("access_key_id")} />
        <BucketField
          label="Secret key"
          value={bucket.secret_access_key}
          onChange={setField("secret_access_key")}
          type="password"
        />
      </div>
      <BucketTest
        check={check}
        isVerified={isBucketVerified}
        canTest={Object.values(bucket).every((value) => value.trim() !== "")}
        onTest={testBucket}
      />
      <SubmitButton isBusy={isBusy} isDisabled={!isComplete || !isBucketVerified} icon={<Sparkles size={15} />}>
        {isBusy ? "Creating your workspace…" : "Create workspace"}
      </SubmitButton>
    </form>
  );
}

type BucketCheck =
  | { kind: "untested" }
  | { kind: "testing" }
  | { kind: "passed"; settingsKey: string }
  | { kind: "failed"; message: string };

/** Creating is only possible once the bucket answered a write, read and delete. */
function BucketTest({
  check,
  isVerified,
  canTest,
  onTest,
}: {
  check: BucketCheck;
  isVerified: boolean;
  canTest: boolean;
  onTest: () => void;
}) {
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-line bg-bg p-2.5">
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          icon={<PlugZap size={13} />}
          isLoading={check.kind === "testing"}
          disabled={!canTest}
          onClick={onTest}
        >
          Test connection
        </Button>
        <AnimatePresence mode="wait" initial={false}>
          {isVerified ? (
            <motion.span key="ok" {...rise} className="flex items-center gap-1 text-sm text-success-text">
              <CircleCheck size={14} /> The bucket works
            </motion.span>
          ) : (
            <motion.span key="hint" {...rise} className="text-xs text-muted">
              {check.kind === "passed" ? "Settings changed; test again." : "Required before creating."}
            </motion.span>
          )}
        </AnimatePresence>
      </div>
      {check.kind === "failed" && (
        <p className="selectable flex gap-1.5 text-xs leading-relaxed text-danger-text">
          <CircleAlert size={14} className="mt-0.5 shrink-0" />
          {check.message}
        </p>
      )}
    </div>
  );
}

function BucketField({
  label,
  value,
  onChange,
  placeholder,
  type = "text",
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: "text" | "password";
}) {
  return (
    <Field label={label}>
      {(id) => (
        <Input
          id={id}
          type={type}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
        />
      )}
    </Field>
  );
}

function JoinForm({ onReady }: { onReady: (session: Session) => void }) {
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

function RestoreForm({ onReady }: { onReady: (session: Session) => void }) {
  const [kit, setKit] = useState("");
  const [isBusy, setBusy] = useState(false);
  const firstInput = useRef<HTMLTextAreaElement>(null);
  useFocusOnMount(firstInput);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      onReady(await api.restoreWorkspace(kit));
    } catch (error) {
      reportError(error);
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-3">
      <p className="flex gap-2 text-sm leading-relaxed text-muted">
        <LifeBuoy size={16} className="mt-0.5 shrink-0" />
        For a workspace admin whose device was lost. Only restore if the old device is gone: two admins at once would
        conflict.
      </p>
      <Field label="Recovery kit">
        {(id) => (
          <TextArea
            id={id}
            ref={firstInput}
            value={kit}
            onChange={(event) => setKit(event.target.value)}
            placeholder="shyrec1_…"
            rows={4}
            className="font-mono text-xs"
          />
        )}
      </Field>
      <SubmitButton isBusy={isBusy} isDisabled={!kit.trim().startsWith("shyrec1_")} icon={<LifeBuoy size={15} />}>
        {isBusy ? "Restoring…" : "Restore admin"}
      </SubmitButton>
    </form>
  );
}

function SubmitButton({
  isBusy,
  isDisabled,
  icon,
  children,
}: {
  isBusy: boolean;
  isDisabled: boolean;
  icon: ReactNode;
  children: ReactNode;
}) {
  return (
    <Button
      type="submit"
      variant="primary"
      size="lg"
      isLoading={isBusy}
      disabled={isDisabled}
      icon={icon}
      className="group mt-2"
    >
      {children}
      {!isBusy && <ArrowRight size={15} className="transition-transform group-hover:translate-x-0.5" />}
    </Button>
  );
}

function Backdrop() {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
      <motion.div
        className="absolute -top-40 left-1/2 h-[520px] w-[820px] -translate-x-1/2 rounded-full opacity-60 blur-3xl"
        style={{ background: "radial-gradient(closest-side, var(--accent-soft), transparent)" }}
        animate={{ scale: [1, 1.08, 1] }}
        transition={{ duration: 9, repeat: Number.POSITIVE_INFINITY, ease: "easeInOut" }}
      />
    </div>
  );
}
