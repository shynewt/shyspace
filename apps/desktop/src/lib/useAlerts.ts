import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { avatarPng } from "./alertPicture";
import { type AlertTarget, api, onChange } from "./api";
import { alertTitle } from "./channels";
import { displayName } from "./people";
import { readPreferences } from "./preferences";
import { reportError } from "./toast";
import { linkedMessageView, type View } from "./workspace";

/**
 * Shows a desktop notification for each alert the core raises (a message, or
 * someone starting a huddle in a DM), unless you're already looking at that
 * channel. Clicking one opens its message, or the DM.
 */
export function useAlerts(view: View | null, navigate: (view: View) => void) {
  const currentView = useRef(view);
  useEffect(() => {
    currentView.current = view;
  }, [view]);

  useEffect(() => {
    // Each alert is prepared at once but shown after the one before, so they
    // appear in the order their messages came, however long each took to prepare.
    let lastShown: Promise<void> = Promise.resolve();
    return onChange((change) => {
      if (change.kind !== "alert" && change.kind !== "huddle_started") return;
      const shown = currentView.current;
      const isLooking = document.hasFocus() && shown?.kind === "channel" && shown.id === change.channel;
      if (isLooking) return;
      const author =
        change.kind === "alert"
          ? { id: change.author, name: change.author_name }
          : { id: change.member, name: change.member_name };
      // A huddle's ring carries no message text, so it shows whatever the text setting.
      const body =
        change.kind === "huddle_started"
          ? "Started a huddle"
          : readPreferences().alertsShowText
            ? change.preview
            : "New message";
      const target: AlertTarget =
        change.kind === "alert" ? { kind: "message", id: change.message } : { kind: "channel", id: change.channel };
      const fallbackName = change.kind === "alert" ? change.channel_name : "";
      const prepared = Promise.all([
        api.channels(),
        api.members(),
        avatarPng(author.id, displayName(author.name, author.id)),
      ]);
      lastShown = lastShown
        .then(() => prepared)
        .then(([channels, members, png]) => {
          const channel = channels.find((candidate) => candidate.id === change.channel);
          const title = alertTitle(author, channel, members, fallbackName);
          return api.showAlert(title, body, target, { member: author.id, png });
        })
        .catch(reportError);
    });
  }, []);

  useEffect(() => {
    const opening = listen<AlertTarget>("alert-opened", (event) => {
      const target = event.payload;
      if (target.kind === "channel") return navigate({ kind: "channel", id: target.id });
      api
        .linkedMessage(target.id)
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
