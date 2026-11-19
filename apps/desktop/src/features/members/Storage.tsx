import { Database } from "lucide-react";
import { type FormEvent, useCallback, useState } from "react";
import { api } from "../../lib/api";
import { reportError, showToast } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { Button, Card, CopyField, Dialog, Field, Input } from "../../ui";

/** The admin's bucket settings: the region and keys can be changed. */
export function Storage() {
  const loadSettings = useCallback(() => api.storageSettings(), []);
  const settings = useLive(loadSettings, (change) => change.kind === "storage");
  const [isEditing, setEditing] = useState(false);
  const [newKit, setNewKit] = useState<string | null>(null);
  if (!settings) return null;
  return (
    <div className="mt-4">
      <Card>
        <div className="flex items-center gap-1.5 text-[13px] font-semibold">
          <Database size={14} /> Storage
        </div>
        <dl className="mt-1.5 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[12px]">
          <dt className="text-muted">Endpoint</dt>
          <dd className="selectable truncate">{settings.endpoint}</dd>
          <dt className="text-muted">Bucket</dt>
          <dd className="selectable truncate">{settings.bucket}</dd>
          <dt className="text-muted">Region</dt>
          <dd className="selectable truncate">{settings.region}</dd>
          <dt className="text-muted">Access key</dt>
          <dd className="selectable truncate font-mono">{settings.access_key_id}</dd>
        </dl>
        <Button className="mt-2" size="sm" onClick={() => setEditing(true)}>
          Change keys
        </Button>
      </Card>
      <UpdateDialog
        key={settings.region}
        open={isEditing}
        initialRegion={settings.region}
        onClose={() => setEditing(false)}
        onUpdated={(kit) => {
          setEditing(false);
          showToast("Storage settings updated. Keep the old key active until everyone has synced.");
          setNewKit(kit);
        }}
      />
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
    </div>
  );
}

function UpdateDialog({
  open,
  initialRegion,
  onClose,
  onUpdated,
}: {
  open: boolean;
  initialRegion: string;
  onClose: () => void;
  onUpdated: (newRecoveryKit: string | null) => void;
}) {
  const [region, setRegion] = useState(initialRegion);
  const [accessKeyId, setAccessKeyId] = useState("");
  const [secretAccessKey, setSecretAccessKey] = useState("");
  const [isSaving, setSaving] = useState(false);
  const isComplete = region.trim() && accessKeyId.trim() && secretAccessKey.trim();

  const save = async (event: FormEvent) => {
    event.preventDefault();
    setSaving(true);
    try {
      onUpdated(await api.updateStorage(region, accessKeyId, secretAccessKey));
      setAccessKeyId("");
      setSecretAccessKey("");
    } catch (error) {
      reportError(error);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title="Change storage keys"
      description="The new keys are tested first, then sent encrypted to every current member. Removed members never get them. Keep the old key active at your provider until everyone has synced."
    >
      <form onSubmit={save} className="flex flex-col gap-3 p-5">
        <Field label="Region">
          {(id) => <Input id={id} value={region} onChange={(event) => setRegion(event.target.value)} />}
        </Field>
        <Field label="Access key">
          {(id) => <Input id={id} value={accessKeyId} onChange={(event) => setAccessKeyId(event.target.value)} />}
        </Field>
        <Field label="Secret key">
          {(id) => (
            <Input
              id={id}
              type="password"
              value={secretAccessKey}
              onChange={(event) => setSecretAccessKey(event.target.value)}
            />
          )}
        </Field>
        <div className="mt-1 flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" isLoading={isSaving} disabled={!isComplete}>
            Test and save
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
