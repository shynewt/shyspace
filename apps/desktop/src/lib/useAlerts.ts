import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { avatarPng } from "./alertPicture";
import { api, onChange } from "./api";
import { alertTitle } from "./channels";
import { displayName } from "./people";
import { readPreferences } from "./preferences";
import { reportError } from "./toast";
import { linkedMessageView, type View } from "./workspace";

/**
 * Shows a desktop notification for each alert the core raises, unless you're
 * already looking at that channel. Clicking one opens its message.
 */
export function useAlerts(view: View | null, navigate: (view: View) => void) {
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
        const author = { id: change.author, name: change.author_name };
        const body = readPreferences().alertsShowText ? change.preview : "New message";
        Promise.all([api.channels(), api.members(), avatarPng(author.id, displayName(author.name, author.id))])
          .then(([channels, members, png]) => {
            const channel = channels.find((candidate) => candidate.id === change.channel);
            const title = alertTitle(author, channel, members, change.channel_name);
            return api.showAlert(title, body, change.message, { member: author.id, png });
          })
          .catch(reportError);
      }),
    [],
  );

  useEffect(() => {
    const opening = listen<string>("alert-opened", (event) => {
      api
        .linkedMessage(event.payload)
        .then((linked) => {
          // A message deleted since its alert has nothing left to open.
          if (linked) navigate(linkedMessageView(linked));
        })
        .catch(reportError);
    });
    return () => {
      opening.then((stop) => stop()).catch(reportError);
    };
  }, [navigate]);
}
