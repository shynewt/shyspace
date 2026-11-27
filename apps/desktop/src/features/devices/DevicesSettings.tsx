import { Laptop, Trash2 } from "lucide-react";
import { useState } from "react";
import { api, type DeviceInfo } from "../../lib/api";
import { useLive } from "../../lib/useLive";
import { Button, IconButton, RelativeTime } from "../../ui";
import { Setting } from "../settings/layout";
import { LinkDeviceDialog } from "./LinkDeviceDialog";
import { deviceName } from "./names";
import { RemoveDeviceDialog } from "./RemoveDeviceDialog";

/** Settings › Devices: your devices, linking a new one, and removing one. */
export function DevicesSettings() {
  const devices = useLive(api.myDevices, (change) => change.kind === "members");
  const [isLinking, setLinking] = useState(false);
  const [removing, setRemoving] = useState<DeviceInfo | null>(null);
  if (!devices) return null;
  const thisDevice = devices.find((device) => device.is_this_device);
  const members = devices.filter((device) => !device.is_linking);
  return (
    <>
      <Setting
        title="Your devices"
        description="Each device has its own key. Remove one you lost, and it stops getting messages once your workspace confirms."
      >
        <ul className="flex flex-col divide-y divide-line">
          {devices.map((device) => (
            <li key={device.id} className="flex items-center gap-3 py-2">
              <Laptop size={16} className="shrink-0 text-decorative" />
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm">{deviceName(device)}</div>
                <div className="text-xs text-muted">
                  <DeviceState device={device} />
                </div>
              </div>
              {mayRemove(device, thisDevice, members.length) ? (
                <IconButton
                  label={device.is_this_device ? "Remove this device" : `Remove ${deviceName(device)}`}
                  icon={<Trash2 size={15} />}
                  onClick={() => setRemoving(device)}
                />
              ) : (
                device.is_first_device &&
                !device.is_this_device && (
                  <span className="text-xs text-muted">Remove this one from that device, or ask your admin.</span>
                )
              )}
            </li>
          ))}
        </ul>
      </Setting>
      <Setting
        title="Link a device"
        description="Use shyspace as you on another computer or phone. You'll compare a number on both."
      >
        <div>
          <Button variant="primary" onClick={() => setLinking(true)}>
            Link a device
          </Button>
        </div>
      </Setting>
      {isLinking && <LinkDeviceDialog onClose={() => setLinking(false)} />}
      {removing && <RemoveDeviceDialog device={removing} devices={devices} onClose={() => setRemoving(null)} />}
    </>
  );
}

/** How a device stands, in one still line. */
function DeviceState({ device }: { device: DeviceInfo }) {
  if (device.is_linking) return <>Linking… It can send once your workspace adds it.</>;
  if (device.is_removing) return <>Removing… finishes when your workspace confirms.</>;
  if (device.is_this_device || device.is_active_now) return <>Active now</>;
  if (device.has_no_activity_seen || device.last_active_ms === null) return <>No activity seen yet</>;
  return (
    <>
      Last active <RelativeTime ms={device.last_active_ms} />
    </>
  );
}

/**
 * Whether this device may ask to remove `device`: not twice, not your only
 * device, and a linked device never your first one.
 */
function mayRemove(device: DeviceInfo, thisDevice: DeviceInfo | undefined, members: number): boolean {
  if (device.is_removing) return false;
  if (device.is_linking) return true;
  if (device.is_this_device) return members > 1;
  return !(device.is_first_device && thisDevice && !thisDevice.is_first_device);
}
