import { KeyRound } from "lucide-react";
import { useState } from "react";
import { api, type StorageSettings } from "../../lib/api";
import { formatSize } from "../../lib/files";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, CopyField, Dialog, DialogActions, FormError, Notice } from "../../ui";

/** How full the recovery snapshot gets before Settings mentions it: years away for most teams. */
const NEARLY_FULL_SHARE = 0.75;

import { ManagedByAdmin } from "./layout";

export function RecoverySettings() {
  const { session } = useWorkspace();
  if (!session.is_admin) {
    return (
      <ManagedByAdmin>Only the admin has a recovery kit. It restores the admin role on a new device.</ManagedByAdmin>
    );
  }
  return <RecoveryKit />;
}

/** The admin's recovery kit: the only way back in if this device is lost. */
function RecoveryKit() {
  const [kit, setKit] = useState<string | null>(null);
  const reveal = useFormAction(async () => setKit(await api.recoveryKit()));
  // Membership changes resize the snapshot.
  const storage = useLive(api.storageSettings, (change) => change.kind === "storage" || change.kind === "members");
  const size = storage ? snapshotSize(storage) : "fits";
  if (size === "too-big" && storage) {
    return (
      <Notice tone="danger" title="Your recovery kit can't restore this workspace">
        <p>
          Your team has outgrown what a recovery kit can hold ({formatSize(storage.recovery_snapshot_limit)}), so a kit
          would no longer bring back your admin role. Until an update fixes this, keep this device safe: it's the only
          one that can add or remove members.
        </p>
      </Notice>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      {size === "nearly-full" && storage?.recovery_snapshot_bytes && (
        <Notice tone="warning" title="Your recovery kit is nearly too big">
          <p>
            It grows as your team grows, and is now {formatSize(storage.recovery_snapshot_bytes)} of the{" "}
            {formatSize(storage.recovery_snapshot_limit)} it can hold. Past that, it can no longer restore your admin
            role. An update to lift this limit is planned.
          </p>
        </Notice>
      )}
      <p className="text-sm leading-relaxed">
        If you lose this device, nobody can add or remove members. Save your recovery kit in a password manager: it
        restores your admin role on a new device.
      </p>
      <FormError message={reveal.error} />
      <Button
        icon={<KeyRound size={14} />}
        isLoading={reveal.isBusy}
        onClick={() => reveal.run()}
        className="self-start"
      >
        Show recovery kit
      </Button>
      <Dialog
        open={kit !== null}
        onOpenChange={(open) => !open && setKit(null)}
        title="Your recovery kit"
        description="Keep it secret and safe. It restores your admin role and holds the storage credentials. Only use it if this device is gone: two admins at once would conflict."
      >
        {kit && <CopyField value={kit} label="Copy recovery kit" />}
        <DialogActions>
          <Button variant="primary" onClick={() => setKit(null)}>
            I saved it
          </Button>
        </DialogActions>
      </Dialog>
    </div>
  );
}

/** Whether the admin's recovery snapshot still fits what restore reads, and whether that's worth a word yet. */
function snapshotSize(storage: StorageSettings): "fits" | "nearly-full" | "too-big" {
  const bytes = storage.recovery_snapshot_bytes;
  if (bytes === null) return "fits";
  if (bytes > storage.recovery_snapshot_limit) return "too-big";
  return bytes >= storage.recovery_snapshot_limit * NEARLY_FULL_SHARE ? "nearly-full" : "fits";
}
