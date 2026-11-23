import { ChevronRight, RefreshCw, ShieldCheck, ShieldQuestion, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { api, type ProtectionCheck } from "../../lib/api";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, FormError, Notice, RelativeTime, Tooltip } from "../../ui";
import { useAdminName } from "./layout";

/** Whether the bucket keeps deleted and overwritten data, as last checked. */
export function useStorageProtection(): ProtectionCheck | undefined {
  return useLive(api.storageProtection, (change) => change.kind === "storage_protection");
}

/** Whether the bucket can lose data, so someone should act. */
export function useStorageNeedsAttention(): boolean {
  return useStorageProtection()?.state === "unprotected";
}

/** A warning sign for wherever the way to Storage starts, while the bucket can lose data. */
export function StorageAttentionSign() {
  if (!useStorageNeedsAttention()) return null;
  return (
    <Tooltip label="Storage can lose data">
      <TriangleAlert
        role="img"
        aria-label="Storage needs attention"
        size={14}
        className="relative ml-auto shrink-0 text-danger"
      />
    </Tooltip>
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
        <p className="flex items-center gap-2 text-sm">
          <ShieldCheck size={16} className="shrink-0 text-success" />
          <span>
            <span className="font-medium text-success-text">Protected.</span> The bucket refuses to delete the
            workspace's data.
          </span>
        </p>
      );
    case "unknown":
      return (
        <p className="flex items-center gap-2 text-sm text-muted">
          <ShieldQuestion size={16} className="shrink-0 text-decorative" />
          Couldn't check yet whether the bucket keeps deleted data.
        </p>
      );
    case "versioned":
      return (
        <Notice tone="warning" title="Partly protected">
          <p>
            Accidents can be undone. If the members' key can't delete old versions (the recommended setup below),
            nothing can be destroyed for good, but shyspace can't check that from here. Otherwise, anyone with the
            bucket keys could still delete old versions.
          </p>
          <WhoFixesIt />
        </Notice>
      );
    case "unprotected":
      return (
        <Notice tone="danger" title="Your storage can lose data">
          <p>
            Anyone with the bucket keys can delete or overwrite your workspace's data, and nothing could bring it back.
            Turn on versioning, and give members a key that can't delete old versions, as below.
          </p>
          <WhoFixesIt />
        </Notice>
      );
  }
}

function WhoFixesIt() {
  const { session } = useWorkspace();
  const adminName = useAdminName();
  if (session.is_admin) return <p className="text-muted">You can fix this at your storage provider.</p>;
  if (!adminName) return null;
  return <p className="text-muted">Ask {adminName} to fix this: only the admin manages the bucket.</p>;
}

/** Steps per provider, naming the workspace's own folders (`prefix` ends in a slash). */
function providerSteps(prefix: string): { provider: string; steps: ReactNode }[] {
  const folder = (name: string) => <code>{`${prefix}${name}`}</code>;
  return [
    {
      provider: "Scaleway (recommended)",
      steps: (
        <>
          Turn on versioning. Give the members' key a bucket policy that allows <code>s3:GetObject</code>,{" "}
          <code>s3:PutObject</code>, <code>s3:DeleteObject</code> and <code>s3:ListBucket</code>, but not{" "}
          <code>s3:DeleteObjectVersion</code>. Add lifecycle rules that delete old versions under {folder("files/")} and{" "}
          {folder("recovery/")} after 30 days, and none for {folder("ops/")} or {folder("mls/")}.
        </>
      ),
    },
    {
      provider: "Cloudflare R2",
      steps: (
        <>
          In the bucket's Settings › Bucket lock rules, add a rule for {folder("ops/")}. Don't lock {folder("mls/")}:
          membership changes repair it by overwriting. R2 keeps no old versions, so back up {folder("files/")} another
          way.
        </>
      ),
    },
    {
      provider: "AWS S3",
      steps: (
        <>
          Turn on versioning, give the members' key no <code>s3:DeleteObjectVersion</code>, and expire old versions
          under {folder("files/")} after 30 days. Don't set an Object Lock default retention: it covers the whole
          bucket.
        </>
      ),
    },
    {
      provider: "MinIO",
      steps: (
        <>
          Run <code>mc version enable</code>, and give members a key whose policy leaves out{" "}
          <code>s3:DeleteObjectVersion</code>.
        </>
      ),
    },
  ];
}

function ProviderSteps() {
  const settings = useLive(api.storageSettings, (change) => change.kind === "storage");
  if (!settings) return null;
  const prefix = `${settings.workspace_id}/`;
  return (
    <details className="group rounded-xl border border-line px-3.5 py-2.5">
      <summary className="flex cursor-default list-none items-center gap-1.5 rounded-md text-sm font-medium [&::-webkit-details-marker]:hidden">
        <ChevronRight size={14} className="text-decorative transition-transform group-open:rotate-90" />
        How to protect your bucket
      </summary>
      <p className="mt-2.5 text-sm leading-relaxed text-muted">
        This workspace keeps everything under <code>{prefix}</code> in the bucket; rules must name that folder.
      </p>
      <dl className="mt-2.5 flex flex-col gap-2 text-sm leading-relaxed">
        {providerSteps(prefix).map(({ provider, steps }) => (
          <div key={provider}>
            <dt className="font-medium">{provider}</dt>
            <dd className="text-muted">{steps}</dd>
          </div>
        ))}
      </dl>
      <p className="mt-2.5 text-sm leading-relaxed">
        <span className="font-medium">Don't lock the whole bucket.</span>{" "}
        <span className="text-muted">
          shyspace deletes and overwrites some of its own files (deleted files, invites, the recovery snapshot, peers),
          so a whole-bucket lock breaks it.
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
        <span className="text-xs text-faint">
          {checkedMs === null ? (
            "Not checked yet"
          ) : (
            <>
              Last checked <RelativeTime ms={checkedMs} />
            </>
          )}
        </span>
      </div>
      <FormError message={check.error} />
    </div>
  );
}
