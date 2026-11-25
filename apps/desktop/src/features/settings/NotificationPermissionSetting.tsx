import { useCallback, useState } from "react";
import { api } from "../../lib/api";
import { useNotificationPermission } from "../../lib/notificationPermission";
import { errorMessage } from "../../lib/toast";
import { Button, FormError } from "../../ui";
import { Setting } from "./layout";

/**
 * Says when the system keeps shyspace's notifications from showing (macOS
 * only), with the way back: its prompt again if it was never answered, or
 * System Settings once it was refused. Shows nothing while they're allowed.
 */
export function NotificationPermissionSetting() {
  // Toasts sit behind the modal Settings window, so failures show here.
  const [error, setError] = useState<string | null>(null);
  const { permission, setPermission } = useNotificationPermission(
    useCallback((failure: unknown) => setError(errorMessage(failure)), []),
  );
  const run = (action: () => Promise<void>) => {
    setError(null);
    action().catch((failure: unknown) => setError(errorMessage(failure)));
  };

  if (permission === "not_asked") {
    return (
      <Setting title="Notifications are off" description="shyspace hasn't been allowed to show notifications yet.">
        <div className="flex items-center gap-3">
          <Button size="sm" onClick={() => run(async () => setPermission(await api.requestNotificationPermission()))}>
            Turn on notifications
          </Button>
        </div>
        <FormError message={error} />
      </Setting>
    );
  }
  if (permission === "denied") {
    return (
      <Setting
        title="macOS is blocking shyspace's notifications"
        description="Allow them for shyspace in System Settings › Notifications."
      >
        <div className="flex items-center gap-3">
          <Button size="sm" onClick={() => run(() => api.openNotificationSettings())}>
            Open System Settings
          </Button>
        </div>
        <FormError message={error} />
      </Setting>
    );
  }
  return <FormError message={error} />;
}
