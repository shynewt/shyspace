import { useCallback, useEffect, useState } from "react";
import { api, type NotificationPermission } from "./api";
import { reportError } from "./toast";

let hasAsked = false;

/**
 * Asks the system for permission to notify (macOS) the first time a
 * workspace is open in this run, if it was never answered: that's when
 * alerts start to matter. Elsewhere the answer is always "allowed".
 */
export function useAskToNotifyOnce() {
  useEffect(() => {
    if (hasAsked) return;
    hasAsked = true;
    api
      .notificationPermission()
      .then((permission) => (permission === "not_asked" ? api.requestNotificationPermission() : permission))
      .catch(reportError);
  }, []);
}

/**
 * The system's answer, read again whenever the window comes back into focus
 * (from System Settings, say). A failed read goes to `onError`.
 */
export function useNotificationPermission(onError: (error: unknown) => void): {
  permission: NotificationPermission | null;
  setPermission: (permission: NotificationPermission) => void;
} {
  const [permission, setPermission] = useState<NotificationPermission | null>(null);
  const read = useCallback(() => {
    api.notificationPermission().then(setPermission, onError);
  }, [onError]);
  useEffect(() => {
    read();
    window.addEventListener("focus", read);
    return () => window.removeEventListener("focus", read);
  }, [read]);
  return { permission, setPermission };
}
