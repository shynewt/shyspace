import { Check } from "lucide-react";
import { type ReactNode, useCallback, useState } from "react";
import { api, type OldKeyState, type StorageProvider, type StorageRotation } from "../../lib/api";
import { openExternal } from "../../lib/contact";
import { useMembers } from "../../lib/members";
import { displayName } from "../../lib/people";
import { showToast } from "../../lib/toast";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, ConfirmDialog, cx, FormError } from "../../ui";

/** The bucket key change a removal started, for the admin; null when none is due. */
export function useStorageRotation(): StorageRotation | null | undefined {
  const { session } = useWorkspace();
  const load = useCallback(
    () => (session.is_admin ? api.storageRotation() : Promise.resolve(null)),
    [session.is_admin],
  );
  return useLive(load, (change) => change.kind === "key_rotation" || change.kind === "storage");
}

type ProviderSteps = { name: string; create: string; remove: string; guide?: { label: string; url: string } };

/** Where each provider makes and deletes keys. Scaleway's menus weren't verified, so its steps stay general. */
const PROVIDER_STEPS: Record<StorageProvider, ProviderSteps> = {
  scaleway: {
    name: "Scaleway",
    create: "In the Scaleway console, create a new API key for the app's IAM application, with access to this bucket.",
    remove: "In the Scaleway console, delete the old API key.",
  },
  r2: {
    name: "Cloudflare",
    create:
      "In the Cloudflare dashboard: R2 object storage › Account Details › Manage next to API Tokens › Create token, with Object Read & Write for this bucket only.",
    remove: "In the same list of API tokens, delete the old one.",
    guide: { label: "Cloudflare's guide", url: "https://developers.cloudflare.com/r2/api/tokens/" },
  },
  b2: {
    name: "Backblaze",
    create:
      "With Backblaze's b2 command-line tool, run: b2 key create --bucket <bucket> <key name> listBuckets,listFiles,readFiles,writeFiles. The web console can only make keys that may delete old versions.",
    remove: "In Application Keys, use Delete Key on the old one.",
    guide: {
      label: "Backblaze's guide",
      url: "https://www.backblaze.com/docs/cloud-storage-create-and-manage-app-keys",
    },
  },
  s3: {
    name: "AWS",
    create: "In AWS: IAM › Users › the app's user › Security credentials › Create access key.",
    remove: "For the old key: Actions › Deactivate, then Delete.",
    guide: {
      label: "AWS's guide",
      url: "https://docs.aws.amazon.com/IAM/latest/UserGuide/id-credentials-access-keys-update.html",
    },
  },
  other: {
    name: "your storage provider",
    create: "At your storage provider, create a new key that can read and write this bucket, and nothing else.",
    remove: "At your storage provider, delete the old key.",
  },
};

/**
 * After a removal, the steps to a storage key the removed member never had:
 * make one, save it, wait for everyone to move to it, delete the old one.
 */
export function KeyRotation() {
  const rotation = useStorageRotation();
  if (!rotation) return null;
  return <RotationSteps rotation={rotation} />;
}

function RotationSteps({ rotation }: { rotation: StorageRotation }) {
  const members = useMembers();
  const [isStopping, setStopping] = useState(false);
  const steps = PROVIDER_STEPS[rotation.provider];
  const nameOf = (id: string) => displayName(members?.find((member) => member.id === id)?.name ?? "", id);
  const waiting = rotation.members.filter((move) => !move.has_moved).length;
  // Their links are signed with the key in use when they were made.
  const invites = useLive(api.invites, (change) => change.kind === "invites");
  const hasPendingInvites = invites?.some((invite) => invite.state === "open" || invite.state === "requested") ?? false;
  const check = useFormAction(async () => {
    const found = await api.checkOldKeyDeleted();
    if (found === "dead") showToast("The old key is deleted. Removed members can't reach the bucket any more.");
  });
  return (
    <section
      aria-labelledby="key-rotation-title"
      className="flex flex-col gap-4 rounded-xl border border-line bg-surface px-4 py-3.5"
    >
      <header>
        <h4 id="key-rotation-title" className="text-sm font-semibold">
          Change the storage key
        </h4>
        <p className="mt-1 text-sm leading-relaxed text-muted">
          A removed member still has the storage key, so they could still reach the bucket. Make a new key, let everyone
          move to it, then delete the old one.
        </p>
      </header>
      <ol className="flex flex-col gap-3.5">
        <Step number={1} isDone={rotation.has_new_key} title={`Create a new key at ${steps.name}`}>
          {steps.create}
          {steps.guide && (
            <>
              {" "}
              <a
                href={steps.guide.url}
                onClick={(event) => {
                  event.preventDefault();
                  if (steps.guide) openExternal(steps.guide.url);
                }}
                className="text-accent-text underline-offset-2 hover:underline"
              >
                {steps.guide.label}
              </a>
            </>
          )}
        </Step>
        <Step number={2} isDone={rotation.has_new_key} title="Save it below">
          Paste the new access key and secret key in the form below. Then save the new recovery kit it gives you: the
          old one stops working once you delete the old key.
        </Step>
        <Step
          number={3}
          isDone={rotation.has_new_key && waiting === 0}
          title={waiting === 0 ? "Everyone has moved to the new key" : "Let everyone move to the new key"}
        >
          {rotation.has_new_key && waiting > 0 && (
            <p className="text-warning-text">
              Deleting the old key now cuts off {waiting} {waiting === 1 ? "person who hasn't" : "people who haven't"}{" "}
              moved yet, until another member is online to pass them the new key. If nobody is, they'd need a new
              invite.
            </p>
          )}
          {rotation.has_new_key && hasPendingInvites && (
            <p className="text-warning-text">
              Deleting the old key also stops invites made before the change working. Make new ones afterwards.
            </p>
          )}
          {rotation.members.length > 0 && (
            <ul aria-label="Moved to the new key" className="mt-1.5 flex flex-col gap-1">
              {rotation.members.map((move) => (
                <li key={move.member} className="flex items-center justify-between gap-3">
                  <span className="truncate text-ink">{nameOf(move.member)}</span>
                  <span className={cx("shrink-0 text-xs", move.has_moved ? "text-success-text" : "text-faint")}>
                    {move.has_moved ? "Moved" : "Not yet"}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Step>
        <Step number={4} isDone={false} title="Delete the old key">
          {steps.remove}
          <OldKeyFinding state={rotation.old_key} />
          <FormError message={check.error} />
          <Button
            size="sm"
            isLoading={check.isBusy}
            disabled={!rotation.has_new_key}
            onClick={() => check.run()}
            className="mt-2 self-start"
          >
            {rotation.old_key === "not_checked" ? "I deleted the old key" : "Check again"}
          </Button>
        </Step>
      </ol>
      <Button variant="ghost" size="sm" onClick={() => setStopping(true)} className="self-start">
        Don't change the key
      </Button>
      <ConfirmDialog
        open={isStopping}
        onOpenChange={setStopping}
        title="Stop changing the storage key?"
        description="The removed member's key may still work, so they could still reach the bucket. The app stops reminding you."
        confirmLabel="Stop"
        onConfirm={() => api.cancelStorageRotation()}
      />
    </section>
  );
}

/** What the last check of the old key found, while it isn't gone. */
function OldKeyFinding({ state }: { state: OldKeyState }) {
  if (state === "still_works") {
    return (
      <p role="alert" className="mt-1 text-danger-text">
        The old key still works. Delete it at the provider, then check again.
      </p>
    );
  }
  if (state === "unknown") {
    return <p className="mt-1">Couldn't reach the bucket to check. The app tries again every 10 minutes.</p>;
  }
  return null;
}

function Step({
  number,
  isDone,
  title,
  children,
}: {
  number: number;
  isDone: boolean;
  title: string;
  children: ReactNode;
}) {
  return (
    <li className="flex gap-3">
      <span
        aria-hidden
        className={cx(
          "mt-px grid size-5 shrink-0 place-items-center rounded-full text-2xs font-semibold",
          isDone ? "bg-success text-on-accent" : "bg-accent-soft text-accent-text",
        )}
      >
        {isDone ? <Check size={12} /> : number}
      </span>
      <div className="flex min-w-0 flex-1 flex-col text-sm leading-relaxed text-muted">
        <p className="font-medium text-ink">
          {title}
          {isDone && <span className="sr-only"> (done)</span>}
        </p>
        {children}
      </div>
    </li>
  );
}
