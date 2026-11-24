import { useEffect, useState } from "react";
import { type AppIcon, api } from "../../lib/api";
import { errorMessage } from "../../lib/toast";
import { FormError, Switch } from "../../ui";
import { Setting } from "./layout";

/**
 * How shyspace shows itself outside its window, on this device. On macOS it
 * lives in the Dock, with an optional menu bar item; on Linux and Windows the
 * tray icon can be a one-colour planet.
 */
export function AppIconSettings() {
  const [icon, setIcon] = useState<AppIcon | null>(null);
  // Toasts sit behind the modal Settings window, so failures show here.
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.appIcon().then(setIcon, (failure: unknown) => setError(errorMessage(failure)));
  }, []);
  if (!icon) return <FormError message={error} />;

  const change = (next: Partial<AppIcon>, save: () => Promise<void>) => {
    const previous = icon;
    setIcon({ ...icon, ...next });
    setError(null);
    save().catch((failure: unknown) => {
      setIcon(previous);
      setError(errorMessage(failure));
    });
  };

  if (icon.platform === "macos") {
    return (
      <Setting
        title="Dock and menu bar"
        description="shyspace stays in the Dock while its window is closed. The menu bar item is a small planet that shows a dot when something needs you."
      >
        <Switch
          label="Show shyspace in the menu bar"
          checked={icon.is_in_menu_bar}
          onChange={(isShown) => change({ is_in_menu_bar: isShown }, () => api.setInMenuBar(isShown))}
        />
        <Switch
          label="Show the unread count on the Dock icon"
          checked={icon.shows_unread_count}
          onChange={(isShown) => change({ shows_unread_count: isShown }, () => api.setShowsUnreadCount(isShown))}
        />
        <FormError message={error} />
      </Setting>
    );
  }
  return (
    <Setting
      title="Tray icon"
      description="A one-colour planet that matches the panel, instead of the app's icon. It shows a dot when something needs you."
    >
      <Switch
        label="Monochrome tray icon"
        checked={icon.is_tray_monochrome}
        onChange={(isMonochrome) =>
          change({ is_tray_monochrome: isMonochrome }, () => api.setTrayMonochrome(isMonochrome))
        }
      />
      <FormError message={error} />
    </Setting>
  );
}
