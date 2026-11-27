import type { DeviceNotice } from "../../lib/api";
import { platformName } from "./names";

/** What a notice about one of your devices says, as a title and a line. */
export function noticeText(notice: DeviceNotice): { title: string; body: string } {
  const device = platformName(notice.platform);
  if (notice.kind === "linked") {
    return {
      title: "A new device can use shyspace as you",
      body: `${device}, linked from another of your devices. If this wasn't you, remove it in Settings › Devices.`,
    };
  }
  return { title: "One of your devices was removed", body: `${device} can no longer read new messages.` };
}

export function noticeKey(notice: DeviceNotice): string {
  return `${notice.device}:${notice.kind}:${notice.at_ms}`;
}
