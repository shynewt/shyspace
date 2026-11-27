import type { DeviceInfo, Platform } from "../../lib/api";

const PLATFORM_NAMES: Record<Platform, string> = {
  Linux: "Linux",
  MacOs: "Mac",
  Windows: "Windows",
  Android: "Android",
  Ios: "iPhone",
};

const linkedDate = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short" });

/** What a device runs on, or "Device" where only the device that linked it knows. */
export function platformName(platform: Platform | null): string {
  return platform ? PLATFORM_NAMES[platform] : "Device";
}

/** A device as the person's own screens name it: "This device", or "Pixel, linked 3 Dec". */
export function deviceName(device: DeviceInfo): string {
  if (device.is_this_device) return "This device";
  if (device.linked_ms === null) return `${platformName(device.platform)}, your first device`;
  return `${platformName(device.platform)}, linked ${linkedDate.format(device.linked_ms)}`;
}
