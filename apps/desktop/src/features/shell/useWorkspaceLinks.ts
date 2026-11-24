import { useMemo } from "react";
import { api } from "../../lib/api";
import type { AppLink, WorkspaceLinks } from "../../lib/appLinks";
import { reportError, showToast } from "../../lib/toast";
import { linkedMessageView, type View } from "../../lib/workspace";
import { useWorkspaces } from "./Workspaces";

/** Reading links in the shown workspace, and opening another workspace's in it. */
export function useWorkspaceLinks(): WorkspaceLinks {
  const { workspaces, shownId, openIn } = useWorkspaces();
  return useMemo(() => {
    const shown = new Map(workspaces.map((workspace) => [workspace.workspace_id, workspace]));
    return {
      workspaceId: shownId,
      workspaceOf: (workspaceId: string) => shown.get(workspaceId),
      openElsewhere: (link: AppLink) => {
        if (!link.workspace) throw new Error("a link that names no workspace opens where it's read");
        openIn(link.workspace, (navigate) => openThere(link, navigate));
      },
    };
  }, [workspaces, shownId, openIn]);
}

function openThere(link: AppLink, navigate: (view: View) => void): void {
  if (link.kind === "page") {
    navigate({ kind: "page", id: link.id });
    return;
  }
  if (link.kind !== "message") throw new Error(`a ${link.kind} link doesn't open anything`);
  api.linkedMessage(link.id).then((linked) => {
    if (!linked) return showToast("That message was deleted.", "info");
    navigate(linkedMessageView(linked));
  }, reportError);
}
