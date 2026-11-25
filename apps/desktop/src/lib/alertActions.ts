import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { type AlertAction, api } from "./api";
import { typedTextToSend } from "./mentions";
import { readPreferences } from "./preferences";
import { shownView } from "./shownView";
import { reportError } from "./toast";
import { type AlertWorkspaces, openAlertTarget } from "./useAlerts";

/**
 * Carries out what was done with notifications: a click opens what it's
 * about, and on macOS a reply is sent where the alerted message is, with its
 * mentions written as the composer writes them, and "Mark as read" reads
 * that conversation. Takes them when one arrives and whenever the open
 * workspaces change, since one may wait for its workspace to open.
 * `openWorkspaces` names them, in any stable form.
 */
export function useAlertActions(openWorkspaces: string, openIn: AlertWorkspaces["openIn"]) {
  const currentOpenIn = useRef(openIn);
  useEffect(() => {
    currentOpenIn.current = openIn;
  }, [openIn]);

  useEffect(() => {
    const take = () => takeAlertActions(currentOpenIn.current);
    // Anything that arrived before the listener was in place is taken once it is.
    const listening = listen("alert-action", take).then((stop) => {
      take();
      return stop;
    });
    return () => {
      listening.then((stop) => stop()).catch(reportError);
    };
  }, []);

  useEffect(() => {
    if (openWorkspaces) takeAlertActions(currentOpenIn.current);
  }, [openWorkspaces]);
}

function takeAlertActions(openIn: AlertWorkspaces["openIn"]) {
  api
    .takeAlertActions()
    .then((actions) => Promise.all(actions.map((action) => carryOut(action, openIn))))
    .catch(reportError);
}

async function carryOut(action: AlertAction, openIn: AlertWorkspaces["openIn"]): Promise<void> {
  if (action.action === "open") {
    const into = action.workspace_id ?? shownView()?.workspaceId;
    if (into) openIn(into, (navigate) => openAlertTarget(action.target, navigate));
    return;
  }
  if (action.action === "mark_read") return api.markAlertRead(action.workspace_id, action.target);
  const { workspace_id: workspaceId, message, text } = action;
  if (action.action === "keep_as_draft") {
    return api.keepUnsentReply(workspaceId, message, text).catch((keeping: unknown) => {
      console.error("a late reply couldn't be kept as a draft", keeping);
      return sayReplyIsLost(workspaceId, text);
    });
  }
  try {
    const { me, members } = await api.peopleIn(workspaceId);
    await api.replyToAlert(workspaceId, message, typedTextToSend(text, workspaceId, members, me));
  } catch (error) {
    console.error("a reply from a notification wasn't sent", error);
    await api.keepUnsentReply(workspaceId, message, text).catch((keeping: unknown) => {
      console.error("an unsent reply couldn't be kept as a draft", keeping);
      return sayReplyIsLost(workspaceId, text);
    });
  }
}

/** The last place a reply that couldn't be sent or kept (its message is gone, say) can still be read, if text may show. */
function sayReplyIsLost(workspaceId: string, text: string) {
  const body = readPreferences().alertsShowText
    ? `It couldn't be kept as a draft either. You wrote: ${text}`
    : "It couldn't be kept as a draft either.";
  return api.showAlert(workspaceId, "Your reply wasn't sent", body, null, null);
}
