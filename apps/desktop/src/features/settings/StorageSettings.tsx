import { useCallback, useState } from "react";
import { api, type StorageSettings as Settings } from "../../lib/api";
import { showToast } from "../../lib/toast";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, CopyField, Dialog, Field, FormError, Input } from "../../ui";
import { ManagedByAdmin } from "./layout";
import { StorageProtection } from "./StorageProtection";

export function StorageSettings() {
  const { session } = useWorkspace();
  return (
    <div className="flex flex-col gap-6">
      <StorageProtection />
      {session.is_admin ? (
        <BucketSettings />
      ) : (
        <ManagedByAdmin>
          Everything is stored encrypted in a bucket the admin chose. The bucket can't read any of it.
        </ManagedByAdmin>
      )}
    </div>
  );
}

function BucketSettings() {
  const loadSettings = useCallback(() => api.storageSettings(), []);
  const settings = useLive(loadSettings, (change) => change.kind === "storage");
  if (!settings) return null;
  return <BucketForm settings={settings} />;
}

/** The admin's bucket settings: the region and keys can change; the endpoint and bucket can't. */
function BucketForm({ settings }: { settings: Settings }) {
  const [region, setRegion] = useState(settings.region);
  const [accessKeyId, setAccessKeyId] = useState(settings.access_key_id);
  const [secretAccessKey, setSecretAccessKey] = useState("");
  const [newKit, setNewKit] = useState<string | null>(null);
  const isComplete = Boolean(region.trim() && accessKeyId.trim() && secretAccessKey.trim());
  const save = useFormAction(async () => {
    const kit = await api.updateStorage(region, accessKeyId, secretAccessKey);
    setSecretAccessKey("");
    showToast("Storage settings saved. Keep the old key active until everyone has synced.");
    setNewKit(kit);
  });

  return (
    <>
      <form onSubmit={save.run} className="flex flex-col gap-3">
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 rounded-xl border border-line bg-bg px-3.5 py-2.5 text-[12.5px]">
          <dt className="text-muted">Endpoint</dt>
          <dd className="selectable truncate">{settings.endpoint}</dd>
          <dt className="text-muted">Bucket</dt>
          <dd className="selectable truncate">{settings.bucket}</dd>
        </dl>
        <p className="text-[12px] leading-relaxed text-muted">
          The endpoint and bucket stay fixed: the workspace's data lives there. You can change the region and keys.
        </p>
        <Field label="Region">
          {(id) => <Input id={id} value={region} onChange={(event) => setRegion(event.target.value)} />}
        </Field>
        <Field label="Access key">
          {(id) => <Input id={id} value={accessKeyId} onChange={(event) => setAccessKeyId(event.target.value)} />}
        </Field>
        <Field
          label="Secret key"
          hint="Needed to save any change. The keys are tested against the bucket first, then sent encrypted to every current member."
        >
          {(id) => (
            <Input
              id={id}
              type="password"
              value={secretAccessKey}
              onChange={(event) => setSecretAccessKey(event.target.value)}
            />
          )}
        </Field>
        <FormError message={save.error} />
        <Button type="submit" variant="primary" isLoading={save.isBusy} disabled={!isComplete} className="self-start">
          Save storage settings
        </Button>
      </form>
      <Dialog
        open={newKit !== null}
        onOpenChange={(open) => !open && setNewKit(null)}
        title="Save your updated recovery kit"
        description="Your recovery kit holds the storage keys, so the old one no longer works. Replace it in your password manager."
      >
        <div className="flex flex-col gap-3 p-5">
          {newKit && <CopyField value={newKit} label="Copy recovery kit" />}
          <Button variant="primary" onClick={() => setNewKit(null)} className="self-end">
            I saved it
          </Button>
        </div>
      </Dialog>
    </>
  );
}
