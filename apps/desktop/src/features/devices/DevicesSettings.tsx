import { Laptop, Monitor, Smartphone, Trash2, X } from "lucide-react";
import { useState } from "react";
import { api, type DeviceInfo, type Platform } from "../../lib/api";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, Card, IconButton, RelativeTime } from "../../ui";
import { Setting } from "../settings/layout";
import { LinkDeviceDialog } from "./LinkDeviceDialog";
import { deviceName } from "./names";
import { RemoveDeviceDialog } from "./RemoveDeviceDialog";

/** Settings › Devices: your devices, linking a new one, and removing one. */
export function DevicesSettings() {
  const devices = useLive(api.myDevices, (change) => change.kind === "members");
  const [isLinking, setLinking] = useState(false);
  const [removing, setRemoving] = useState<DeviceInfo | null>(null);
  const [hasRemoved, setRemoved] = useState(false);
  if (!devices) return null;
  return (
    <>
      {hasRemoved && <AfterRemoving onDismiss={() => setRemoved(false)} />}
      <Setting
        title="Your devices"
        description="Each device has its own key. Remove one you lost, and it stops getting messages once your workspace confirms."
      >
        <ul className="flex flex-col divide-y divide-line">
          {devices.map((device) => (
            <li key={device.id} className="flex items-center gap-3 py-2">
              <PlatformIcon platform={device.platform} />
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm">{deviceName(device)}</div>
                <div className="text-xs text-muted">
                  <DeviceState device={device} />
                </div>
              </div>
              <RemoveControl device={device} onRemove={() => setRemoving(device)} />
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
      {removing && (
        <RemoveDeviceDialog
          device={removing}
          devices={devices}
          onClose={() => setRemoving(null)}
          onRemoved={() => setRemoved(!removing.is_linking)}
        />
      )}
    </>
  );
}

const PHONES: Platform[] = ["Android", "Ios"];

function PlatformIcon({ platform }: { platform: Platform | null }) {
  const className = "shrink-0 text-decorative";
  if (platform === null) return <Monitor size={16} className={className} />;
  if (PHONES.includes(platform)) return <Smartphone size={16} className={className} />;
  return <Laptop size={16} className={className} />;
}

/** Remove, or why not: the core says which, by the rules a removal checks. */
function RemoveControl({ device, onRemove }: { device: DeviceInfo; onRemove: () => void }) {
  if (device.may_remove) {
    return (
      <IconButton
        label={device.is_this_device ? "Remove this device" : `Remove ${deviceName(device)}`}
        icon={<Trash2 size={15} />}
        onClick={onRemove}
      />
    );
  }
  // This device's own reason (your only one, reading only) is plain from the rest of the screen.
  if (device.is_this_device || device.is_removing || !device.why_not_removable) return null;
  return <span className="max-w-48 text-right text-xs text-muted">{device.why_not_removable}</span>;
}

/** How a device stands, in one still line. */
function DeviceState({ device }: { device: DeviceInfo }) {
  if (device.is_linking) return <>Linking… It can send once your workspace adds it.</>;
  if (device.is_removing)
    return <>Removing… finishes when your workspace confirms. Until then it can still read and send.</>;
  if (device.is_this_device || device.is_active_now) return <>Active now</>;
  if (device.has_no_activity_seen || device.last_active_ms === null) return <>No activity seen yet</>;
  return (
    <>
      Last active <RelativeTime ms={device.last_active_ms} />
    </>
  );
}

/** What to do after removing a lost device, one tap each; it can be dismissed. */
function AfterRemoving({ onDismiss }: { onDismiss: () => void }) {
  const { openSettings, session } = useWorkspace();
  return (
    <div className="mb-4">
      <Card>
        <div className="flex items-start gap-2">
          <div className="flex-1">
            <p className="text-sm font-medium">If that device was lost</p>
            <p className="mt-0.5 text-xs leading-relaxed text-muted">
              Its last name, picture and status stay until you set them again.
              {session.is_admin && " It also knew the storage keys."}
            </p>
            <div className="mt-2 flex flex-wrap gap-2">
              <Button size="sm" variant="secondary" onClick={() => openSettings({ section: "profile" })}>
                Set your name, picture and status again
              </Button>
              {session.is_admin && (
                <Button size="sm" variant="secondary" onClick={() => openSettings({ section: "storage" })}>
                  Change the storage keys
                </Button>
              )}
            </div>
          </div>
          <IconButton label="Dismiss" size="sm" icon={<X size={14} />} onClick={onDismiss} />
        </div>
      </Card>
    </div>
  );
}
