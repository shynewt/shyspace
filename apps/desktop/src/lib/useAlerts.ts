import { useEffect, useRef } from "react";
import { avatarPng } from "./alertPicture";
import { type AlertKind, type AlertTarget, api, type Change, onAnyWorkspaceChange } from "./api";
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

type AlertChange = Extract<Change, { kind: "alert" } | { kind: "page_alert" } | { kind: "huddle_started" }>;
type ChannelAlertChange = Exclude<AlertChange, { kind: "page_alert" }>;

/**
 * Shows a desktop notification for each alert the core raises in any
 * workspace (a message, a mention in a page, or someone starting a huddle in
 * a DM), unless you're already looking at that channel or page. Clicking one
 * opens its message, the page at the mention, or the DM, switching workspace
 * first if it's in another (see `useAlertActions`). Runs at the app's level, so
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
      if (workspaceId === null || !isAlert(change)) return;
      const { names } = currentWorkspaces.current;
      const shown = shownView();
      const isShown = workspaceId === shown?.workspaceId;
      const view = shown?.view();
      const isLooking = isShown && document.hasFocus() && !!view && isAbout(change, view);
      if (isLooking) return;
      const author = authorOf(change);
      const prepared = Promise.all([
        change.kind === "page_alert"
          ? Promise.resolve(pageAlertTitle(change))
          : isShown
            ? titleInShown(change)
            : Promise.resolve(titleElsewhere(change)),
        avatarPng(author.id, displayName(author.name, author.id), { canLoadPicture: isShown }),
      ]);
      const workspaceName = names.size > 1 ? names.get(workspaceId) : undefined;
      lastShown = lastShown
        .then(() => prepared)
        .then(([title, png]) =>
          api.showAlert(
            alertKindOf(change),
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
}

function alertKindOf(change: AlertChange): AlertKind {
  switch (change.kind) {
    case "alert":
      return "message";
    case "page_alert":
      return "page_mention";
    case "huddle_started":
      return "huddle";
  }
}

/** Opens what a notification is about: its message, its channel, or the spot in its page. */
export function openAlertTarget(target: AlertTarget, navigate: (view: View) => void) {
  if (target.kind === "channel") return navigate({ kind: "channel", id: target.id });
  if (target.kind === "page") return navigate({ kind: "page", id: target.id, at: target.position });
  api
    .linkedMessage(target.id)
    .then((linked) => {
      // A message deleted since its alert has nothing left to open.
      if (linked) navigate(linkedMessageView(linked));
    })
    .catch(reportError);
}

function isAlert(change: Change): change is AlertChange {
  return change.kind === "alert" || change.kind === "page_alert" || change.kind === "huddle_started";
}

function isAbout(change: AlertChange, view: View): boolean {
  if (change.kind === "page_alert") return view.kind === "page" && view.id === change.page;
  return view.kind === "channel" && view.id === change.channel;
}

function authorOf(change: AlertChange) {
  return change.kind === "huddle_started"
    ? { id: change.member, name: change.member_name }
    : { id: change.author, name: change.author_name };
}

/** A huddle's ring carries no message text, so it shows whatever the text setting. */
function bodyOf(change: AlertChange): string {
  if (change.kind === "huddle_started") return "Started a huddle";
  const showsText = readPreferences().alertsShowText;
  if (change.kind === "page_alert") return showsText ? change.preview : "Mentioned you in a page";
  return showsText ? change.preview : "New message";
}

function targetOf(change: AlertChange): AlertTarget {
  if (change.kind === "alert") return { kind: "message", id: change.message };
  if (change.kind === "page_alert") return { kind: "page", id: change.page, position: change.position };
  return { kind: "channel", id: change.channel };
}

/** "Grace in Plan", or with the text hidden only "Grace": a page's title can say as much as its text. */
function pageAlertTitle(change: Extract<AlertChange, { kind: "page_alert" }>): string {
  const name = displayName(change.author_name, change.author);
  return readPreferences().alertsShowText ? `${name} in ${change.page_title || "Untitled"}` : name;
}

/** "Grace in #general", or for a DM who it's from and with whom, from the shown workspace's channels. */
async function titleInShown(change: ChannelAlertChange): Promise<string> {
  const [channels, members] = await Promise.all([api.channels(), api.members()]);
  const channel = channels.find((candidate) => candidate.id === change.channel);
  return alertTitle(authorOf(change), channel, members, change.kind === "alert" ? change.channel_name : "");
}

/** The same for another workspace, from what the alert itself carries: its channel's name, empty for a DM. */
function titleElsewhere(change: ChannelAlertChange): string {
  const author = authorOf(change);
  const name = displayName(author.name, author.id);
  const channelName = change.kind === "alert" ? change.channel_name : "";
  return channelName ? `${name} in #${channelName}` : name;
}
