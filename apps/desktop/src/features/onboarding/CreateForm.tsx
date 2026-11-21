import { CircleAlert, CircleCheck, Lock, PlugZap, Sparkles } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type FormEvent, useRef, useState } from "react";
import { api, type BucketConfig, type Session } from "../../lib/api";
import { MAX_PROFILE_NAME_CHARS } from "../../lib/limits";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Button, Field, Input, rise } from "../../ui";
import { SubmitButton } from "./SubmitButton";

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

export function CreateForm({ onReady }: { onReady: (session: Session) => void }) {
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
