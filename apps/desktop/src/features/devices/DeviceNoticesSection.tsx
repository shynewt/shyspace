import { MonitorSmartphone } from "lucide-react";
import { useState } from "react";
import { api } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, RelativeTime, SectionLabel } from "../../ui";
import { noticeKey, noticeText } from "./notices";

/** Unseen notices about your devices, at the top of the inbox until you've seen them. */
export function DeviceNoticesSection() {
  const { openSettings } = useWorkspace();
  const notices = useLive(api.deviceNotices, (change) => change.kind === "members");
  const [isBusy, setBusy] = useState(false);
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
    </section>
  );
}
