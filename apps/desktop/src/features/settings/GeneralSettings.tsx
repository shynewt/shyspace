import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { errorMessage } from "../../lib/toast";
import { FormError, Switch } from "../../ui";
import { AppIconSettings } from "./AppIconSettings";
import { Setting } from "./layout";

/** How shyspace runs on this device: at login, and how it shows outside its window. */
export function GeneralSettings() {
  return (
    <>
      <OpenAtLoginSetting />
      <AppIconSettings />
    </>
  );
}

/**
 * The system's own record decides, so it's read again whenever the window
 * comes back into focus: it can be changed in the system's settings too.
 */
function OpenAtLoginSetting() {
  const [isOn, setIsOn] = useState<boolean | null>(null);
  // Toasts sit behind the modal Settings window, so failures show here.
  const [error, setError] = useState<string | null>(null);
  const read = useCallback(() => api.openAtLogin().then(setIsOn), []);
  // A fresh read on coming back (from the system's settings, say) replaces an old error.
  const refresh = useCallback(() => {
    read().then(
      () => setError(null),
      (failure: unknown) => setError(errorMessage(failure)),
    );
  }, [read]);
  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  const change = (next: boolean) => {
    setIsOn(next);
    setError(null);
    api.setOpenAtLogin(next).then(refresh, (failure: unknown) => {
      setError(errorMessage(failure));
      read().catch((reading: unknown) => setError(errorMessage(reading)));
    });
  };

  return (
    <Setting
      title="Open at login"
      description="shyspace starts in the background when you log in, so alerts reach you without opening it first."
    >
      {isOn !== null && <Switch label="Open shyspace when you log in" checked={isOn} onChange={change} />}
      <FormError message={error} />
    </Setting>
  );
}
