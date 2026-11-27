import { useState } from "react";
import { api, type DeviceInfo } from "../../lib/api";
import { reportError, showToast } from "../../lib/toast";
import { Button, Dialog, DialogActions } from "../../ui";
import { deviceName } from "./names";

/**
 * Asks before removing one of your devices. If it linked others, it names
 * them: removing only this one stays the main button, removing those too the
 * quieter one.
 */
export function RemoveDeviceDialog({
  device,
  devices,
  onClose,
  onRemoved,
}: {
  device: DeviceInfo;
  devices: DeviceInfo[];
  onClose: () => void;
  onRemoved?: () => void;
}) {
  const [isBusy, setBusy] = useState(false);
  const name = deviceName(device);
  const linked = devices.filter((candidate) => device.linked_devices.includes(candidate.id));
  const remove = async (withLinked: boolean) => {
    setBusy(true);
    try {
      if (withLinked) await api.removeMyDeviceAndItsLinks(device.id);
      else await api.removeMyDevice(device.id);
      showToast(device.is_linking ? "The link was stopped." : "Removing… finishes when your workspace confirms.");
      onRemoved?.();
      onClose();
    } catch (error) {
      reportError(error);
    } finally {
      setBusy(false);
    }
  };
  const title = device.is_this_device ? "Remove this device?" : `Remove "${name}"?`;
  const description = device.is_linking
    ? "It isn't added yet. Removing it stops the link."
    : "Once your workspace confirms, it stops getting messages and can't send as you.";
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()} title={title} description={description} width={420}>
      {device.has_no_activity_seen && <p className="text-sm text-muted">It hasn't been seen yet.</p>}
      {linked.length > 0 && <p className="text-sm">It linked: {linked.map(deviceName).join(", ")}</p>}
      <DialogActions>
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        {linked.length > 0 && (
          <Button variant="secondary" disabled={isBusy} onClick={() => remove(true)}>
            Also remove the devices it linked
          </Button>
        )}
        <Button variant="danger" isLoading={isBusy} onClick={() => remove(false)}>
          {linked.length > 0 ? "Remove this one" : "Remove"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
