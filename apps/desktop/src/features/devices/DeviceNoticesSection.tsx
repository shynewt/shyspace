import { MonitorSmartphone } from "lucide-react";
import { useState } from "react";
import { api, type DeviceInfo, type DeviceNotice } from "../../lib/api";
import { reportError, showToast } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, RelativeTime, SectionLabel } from "../../ui";
import { noticeKey, noticeText } from "./notices";
import { RemoveDeviceDialog } from "./RemoveDeviceDialog";

/** Unseen notices about your devices, at the top of the inbox until you've seen them. */
export function DeviceNoticesSection() {
  const { openSettings } = useWorkspace();
  const notices = useLive(api.deviceNotices, (change) => change.kind === "members");
  const [isBusy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<{ device: DeviceInfo; devices: DeviceInfo[] } | null>(null);
  const unseen = notices?.filter((notice) => !notice.is_seen) ?? [];
  if (unseen.length === 0) return null;
  const markSeen = async () => {
    setBusy(true);
    try {
      await api.markDeviceNoticesSeen();
    } catch (error) {
      reportError(error);
    } finally {
      setBusy(false);
    }
  };
  // Straight to the confirm for that device: one step from the notice.
  const removeLinked = async (notice: DeviceNotice) => {
    try {
      const devices = await api.myDevices();
      const device = devices.find((candidate) => candidate.id === notice.device);
      if (!device) return showToast("That device isn't one of yours any more.", "info");
      if (!device.may_remove) return showToast(device.why_not_removable ?? "It can't be removed from here.", "info");
      setRemoving({ device, devices });
    } catch (error) {
      reportError(error);
    }
  };
  return (
    <section aria-label="Your devices" className="mb-6">
      <SectionLabel>Your devices</SectionLabel>
      <ul className="flex flex-col gap-2">
        {unseen.map((notice) => {
          const { title, body } = noticeText(notice);
          return (
            <li key={noticeKey(notice)} className="flex gap-3 rounded-md border border-line px-3 py-2">
              <MonitorSmartphone size={16} className="mt-0.5 shrink-0 text-decorative" />
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline gap-2">
                  <span className="text-sm font-medium">{title}</span>
                  <RelativeTime ms={notice.at_ms} className="text-xs text-muted" />
                </div>
                <p className="text-xs text-muted">{body}</p>
              </div>
              {notice.kind === "linked" && (
                <Button size="sm" variant="secondary" onClick={() => void removeLinked(notice)}>
                  Not you? Remove it
                </Button>
              )}
            </li>
          );
        })}
      </ul>
      <div className="mt-2 flex gap-2">
        <Button variant="secondary" onClick={() => openSettings({ section: "devices" })}>
          Review devices
        </Button>
        <Button variant="ghost" isLoading={isBusy} onClick={markSeen}>
          Got it
        </Button>
      </div>
      {removing && (
        <RemoveDeviceDialog device={removing.device} devices={removing.devices} onClose={() => setRemoving(null)} />
      )}
    </section>
  );
}
