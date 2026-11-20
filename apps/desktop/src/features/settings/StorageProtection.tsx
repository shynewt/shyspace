import { ChevronRight, RefreshCw, ShieldAlert, ShieldCheck, ShieldQuestion } from "lucide-react";
import type { ReactNode } from "react";
import { api, type ProtectionCheck } from "../../lib/api";
import { formatWhen } from "../../lib/people";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, cx, FormError } from "../../ui";
import { useAdminName } from "./layout";

/** Whether the bucket keeps deleted and overwritten data, as last checked. */
export function useStorageProtection(): ProtectionCheck | undefined {
  return useLive(api.storageProtection, (change) => change.kind === "storage_protection");
}

/** A dot for wherever the way to Storage starts, while the bucket can lose data. */
export function StorageAttentionDot() {
  const protection = useStorageProtection();
  if (protection?.state !== "unprotected") return null;
  return (
    <span
      role="img"
      aria-label="Storage needs attention"
      className="relative ml-auto h-2 w-2 shrink-0 rounded-full bg-danger"
    />
  );
}

/** How safe the workspace's data is in the bucket, how to fix it, and a way to check again. */
export function StorageProtection() {
  const protection = useStorageProtection();
  if (!protection) return null;
  return (
    <section aria-label="Storage protection" className="flex flex-col gap-3">
      <ProtectionNotice protection={protection} />
      {protection.state !== "locked" && <ProviderSteps />}
      <CheckAgain checkedMs={protection.checked_ms} />
    </section>
  );
}

function ProtectionNotice({ protection }: { protection: ProtectionCheck }) {
  switch (protection.state) {
    case "locked":
      return (
        <p className="flex items-center gap-2 text-[13px]">
          <ShieldCheck size={16} className="shrink-0 text-success" />
          <span>
            <span className="font-medium text-success-text">Protected.</span> The bucket refuses to delete the
            workspace's data.
          </span>
        </p>
      );
    case "unknown":
      return (
        <p className="flex items-center gap-2 text-[13px] text-muted">
          <ShieldQuestion size={16} className="shrink-0 text-decorative" />
          Couldn't check yet whether the bucket keeps deleted data.
        </p>
      );
    case "versioned":
      return (
        <Notice tone="warning" title="Partly protected">
          <p>
            Accidents can be undone, but anyone with the bucket keys could still delete old versions. For full
            protection, add a retention lock on the <code>ops/</code> and <code>mls/</code> folders (Cloudflare R2
            bucket lock, or S3 Object Lock). If you already use S3 Object Lock, you can ignore this: shyspace can't tell
            it apart from versioning.
          </p>
          <WhoFixesIt />
        </Notice>
      );
    case "unprotected":
      return (
        <Notice tone="danger" title="Your storage can lose data">
          <p>
            Anyone with the bucket keys can delete or overwrite your workspace's data, and nothing could bring it back.
          </p>
          <ol className="list-decimal pl-5">
            <li>
              Best: add a retention lock on the <code>ops/</code> and <code>mls/</code> folders, so nothing there can be
              deleted.
            </li>
            <li>If your provider has no locks, turn on versioning, so old data can be brought back.</li>
          </ol>
          <WhoFixesIt />
        </Notice>
      );
  }
}

function Notice({ tone, title, children }: { tone: "warning" | "danger"; title: string; children: ReactNode }) {
  return (
    <div
      className={cx(
        "flex gap-3 rounded-xl border p-3.5",
        tone === "danger" ? "border-danger/40 bg-danger/8" : "border-warning/40 bg-warning/8",
      )}
    >
      <ShieldAlert size={18} className={cx("mt-px shrink-0", tone === "danger" ? "text-danger" : "text-warning")} />
      <div className="flex min-w-0 flex-col gap-1.5 text-[12.5px] leading-relaxed">
        <h4 className={cx("text-[13.5px] font-semibold", tone === "danger" ? "text-danger-text" : "text-warning-text")}>
          {title}
        </h4>
        {children}
      </div>
    </div>
  );
}

function WhoFixesIt() {
  const { session } = useWorkspace();
  const adminName = useAdminName();
  if (session.is_admin) return <p className="text-muted">You can fix this at your storage provider.</p>;
  if (!adminName) return null;
  return <p className="text-muted">Ask {adminName} to fix this: only the admin manages the bucket.</p>;
}

const PROVIDER_STEPS: { provider: string; steps: ReactNode }[] = [
  {
    provider: "Cloudflare R2",
    steps: (
      <>
        In the bucket's Settings › Bucket lock rules, add a rule for the prefix <code>ops/</code> and one for{" "}
        <code>mls/</code>.
      </>
    ),
  },
  { provider: "AWS S3", steps: "Turn on Versioning, then set an Object Lock default retention." },
  { provider: "Backblaze B2", steps: "In Lifecycle settings, choose “Keep all versions” (this gives versioning)." },
  {
    provider: "MinIO",
    steps: (
      <>
        Run <code>mc version enable</code> for versioning, or turn on object lock when creating the bucket.
      </>
    ),
  },
];

function ProviderSteps() {
  return (
    <details className="group rounded-xl border border-line px-3.5 py-2.5">
      <summary className="flex cursor-default list-none items-center gap-1.5 rounded-md text-[13px] font-medium [&::-webkit-details-marker]:hidden">
        <ChevronRight size={14} className="text-decorative transition-transform group-open:rotate-90" />
        How to protect your bucket
      </summary>
      <dl className="mt-2.5 flex flex-col gap-2 text-[12.5px] leading-relaxed">
        {PROVIDER_STEPS.map(({ provider, steps }) => (
          <div key={provider}>
            <dt className="font-medium">{provider}</dt>
            <dd className="text-muted">{steps}</dd>
          </div>
        ))}
      </dl>
      <p className="mt-2.5 text-[12.5px] leading-relaxed">
        <span className="font-medium">Don't lock the whole bucket.</span>{" "}
        <span className="text-muted">
          shyspace deletes and overwrites some of its own files (invites, the recovery snapshot, peers), so a
          whole-bucket lock breaks it. Lock only <code>ops/</code> and <code>mls/</code>.
        </span>
      </p>
    </details>
  );
}

function CheckAgain({ checkedMs }: { checkedMs: number | null }) {
  const check = useFormAction(async () => {
    await api.checkStorageProtection();
  });
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-3">
        <Button
          size="sm"
          icon={<RefreshCw size={13} />}
          isLoading={check.isBusy}
          onClick={() => check.run()}
          className="self-start"
        >
          Check again
        </Button>
        <span className="text-[12px] text-faint">
          {checkedMs === null ? "Not checked yet" : `Last checked ${formatWhen(checkedMs, new Date())}`}
        </span>
      </div>
      <FormError message={check.error} />
    </div>
  );
}
