import { KeyRound, LifeBuoy } from "lucide-react";
import { useState } from "react";
import { api } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { Button, Card, CopyField, Dialog } from "../../ui";

/** The admin's recovery kit: the only way back in if this device is lost. */
export function Recovery() {
  const [kit, setKit] = useState<string | null>(null);
  const [isLoading, setLoading] = useState(false);
  const reveal = async () => {
    setLoading(true);
    try {
      setKit(await api.recoveryKit());
    } catch (error) {
      reportError(error);
    } finally {
      setLoading(false);
    }
  };
  return (
    <div className="mt-4">
      <Card>
        <div className="flex items-center gap-1.5 text-[13px] font-semibold">
          <LifeBuoy size={14} /> Admin recovery
        </div>
        <p className="mt-0.5 text-[12px] leading-relaxed text-muted">
          If you lose this device, nobody can add or remove members. Save your recovery kit in a password manager to
          restore your admin role on a new device.
        </p>
        <Button className="mt-2" size="sm" icon={<KeyRound size={13} />} isLoading={isLoading} onClick={reveal}>
          Show recovery kit
        </Button>
      </Card>
      <Dialog
        open={kit !== null}
        onOpenChange={(open) => !open && setKit(null)}
        title="Your recovery kit"
        description="Keep it secret and safe. It restores your admin role and holds the storage credentials. Only use it if this device is gone: two admins at once would conflict."
      >
        <div className="flex flex-col gap-3 p-5">
          {kit && <CopyField value={kit} label="Copy recovery kit" />}
          <Button variant="primary" onClick={() => setKit(null)} className="self-end">
            I saved it
          </Button>
        </div>
      </Dialog>
    </div>
  );
}
