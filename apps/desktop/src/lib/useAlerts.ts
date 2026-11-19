import { useEffect, useRef } from "react";
import { api, onChange } from "./api";
import { displayName } from "./people";
import { readPreferences } from "./preferences";
import { reportError } from "./toast";
import type { View } from "./workspace";

/**
 * Shows a desktop notification for each alert the core raises, unless you're
 * already looking at that channel.
 */
export function useAlerts(view: View | null) {
  const currentView = useRef(view);
  useEffect(() => {
    currentView.current = view;
  }, [view]);

  useEffect(
    () =>
      onChange((change) => {
        if (change.kind !== "alert") return;
        const shown = currentView.current;
        const isLooking = document.hasFocus() && shown?.kind === "channel" && shown.id === change.channel;
        if (isLooking) return;
        const title = `${displayName(change.author_name, change.author)} in #${change.channel_name}`;
        const body = readPreferences().alertsShowText ? change.preview : "New message";
        api.showAlert(title, body).catch(reportError);
      }),
    [],
  );
}
