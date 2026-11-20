import { KeyRound } from "lucide-react";
import { useState } from "react";
import { api } from "../../lib/api";
import { useFormAction } from "../../lib/useFormAction";
import { useWorkspace } from "../../lib/workspace";
import { Button, CopyField, Dialog, DialogActions, FormError } from "../../ui";
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
  return (
    <div className="flex flex-col gap-3">
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
