import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { avatarPng } from "./alertPicture";
import { type AlertTarget, api, type Change, onAnyWorkspaceChange } from "./api";
import { alertTitle } from "./channels";
import { displayName } from "./people";
import { readPreferences } from "./preferences";
import { shownView } from "./shownView";
import { reportError } from "./toast";
import { linkedMessageView, type View } from "./workspace";

/** The workspaces on this device, as notifications need them. */
export type AlertWorkspaces = {
  /** Names by workspace id; a notification names its workspace once there are two or more. */
  names: ReadonlyMap<string, string>;
  /** Opens something in a workspace, showing it first if it isn't (even the selected one, while adding another). */
  openIn: (workspaceId: string, open: (navigate: (view: View) => void) => void) => void;
};

type AlertChange = Extract<Change, { kind: "alert" } | { kind: "huddle_started" }>;

/**
 * Shows a desktop notification for each alert the core raises in any
 * workspace (a message, or someone starting a huddle in a DM), unless you're
 * already looking at that channel. Clicking one opens its message, or the DM,
 * switching workspace first if it's in another. Runs at the app's level, so
 * it keeps going while no workspace is shown, as while adding one.
 */
export function useAlerts(workspaces: AlertWorkspaces) {
  const currentWorkspaces = useRef(workspaces);
  useEffect(() => {
    currentWorkspaces.current = workspaces;
  }, [workspaces]);

  useEffect(() => {
    // Each alert is prepared at once but shown after the one before, so they
    // appear in the order their messages came, however long each took to prepare.
    let lastShown: Promise<void> = Promise.resolve();
    return onAnyWorkspaceChange((workspaceId, change) => {
      if (workspaceId === null || (change.kind !== "alert" && change.kind !== "huddle_started")) return;
      const { names } = currentWorkspaces.current;
      const shown = shownView();
      const isShown = workspaceId === shown?.workspaceId;
      const view = shown?.view();
      const isLooking = isShown && document.hasFocus() && view?.kind === "channel" && view.id === change.channel;
      if (isLooking) return;
      const author = authorOf(change);
      const prepared = Promise.all([
        isShown ? titleInShown(change) : Promise.resolve(titleElsewhere(change)),
        avatarPng(author.id, displayName(author.name, author.id)),
      ]);
      const workspaceName = names.size > 1 ? names.get(workspaceId) : undefined;
      lastShown = lastShown
        .then(() => prepared)
        .then(([title, png]) =>
          api.showAlert(
            workspaceId,
            workspaceName ? `${workspaceName} · ${title}` : title,
            bodyOf(change),
            targetOf(change),
            { member: author.id, png },
          ),
        )
        .catch(reportError);
    });
  }, []);

  useEffect(() => {
    const opening = listen<{ workspace_id: string | null } & AlertTarget>("alert-opened", (event) => {
      const { workspace_id: workspaceId, ...target } = event.payload;
      const into = workspaceId ?? shownView()?.workspaceId;
      if (into) currentWorkspaces.current.openIn(into, (navigate) => openAlertTarget(target, navigate));
    });
    return () => {
      opening.then((stop) => stop()).catch(reportError);
    };
  }, []);
}

/** Opens what a notification is about: its message, or its channel. */
export function openAlertTarget(target: AlertTarget, navigate: (view: View) => void) {
  if (target.kind === "channel") return navigate({ kind: "channel", id: target.id });
  api
    .linkedMessage(target.id)
    .then((linked) => {
      // A message deleted since its alert has nothing left to open.
      if (linked) navigate(linkedMessageView(linked));
    })
    .catch(reportError);
}

function authorOf(change: AlertChange) {
  return change.kind === "alert"
    ? { id: change.author, name: change.author_name }
    : { id: change.member, name: change.member_name };
}

/** A huddle's ring carries no message text, so it shows whatever the text setting. */
function bodyOf(change: AlertChange): string {
  if (change.kind === "huddle_started") return "Started a huddle";
  return readPreferences().alertsShowText ? change.preview : "New message";
}

function targetOf(change: AlertChange): AlertTarget {
  return change.kind === "alert" ? { kind: "message", id: change.message } : { kind: "channel", id: change.channel };
}

/** "Grace in #general", or for a DM who it's from and with whom, from the shown workspace's channels. */
async function titleInShown(change: AlertChange): Promise<string> {
  const [channels, members] = await Promise.all([api.channels(), api.members()]);
  const channel = channels.find((candidate) => candidate.id === change.channel);
  return alertTitle(authorOf(change), channel, members, change.kind === "alert" ? change.channel_name : "");
}

/** The same for another workspace, from what the alert itself carries: its channel's name, empty for a DM. */
function titleElsewhere(change: AlertChange): string {
  const author = authorOf(change);
  const name = displayName(author.name, author.id);
  const channelName = change.kind === "alert" ? change.channel_name : "";
  return channelName ? `${name} in #${channelName}` : name;
}
