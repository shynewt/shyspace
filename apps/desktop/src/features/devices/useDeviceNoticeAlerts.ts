import { useEffect, useRef } from "react";
import { api, onAnyWorkspaceChange } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { noticeKey, noticeText } from "./notices";

/**
 * Shows a desktop notification for each notice about your devices that
 * arrives while the app runs, in every open workspace, shown or not: the
 * notice is the defense against someone linking or removing devices from
 * one they hold. Ones already there when a workspace opened only show in
 * its inbox.
 */
export function useDeviceNoticeAlerts(workspaceIds: string[]) {
  // Per workspace, the notices already known; missing until its first load.
  const known = useRef(new Map<string, Set<string>>());
  const idList = workspaceIds.join(" ");

  useEffect(() => {
    const ids = idList.split(" ").filter(Boolean);
    const check = async (workspaceId: string) => {
      const notices = await api.workspaceDeviceNotices(workspaceId);
      const before = known.current.get(workspaceId);
      const seen = before ?? new Set<string>();
      known.current.set(workspaceId, seen);
      for (const notice of notices) {
        const key = noticeKey(notice);
        if (seen.has(key)) continue;
        seen.add(key);
        if (!before || notice.is_seen) continue;
        const { title, body } = noticeText(notice);
        await api.showAlert("notice", workspaceId, title, body, null, null);
      }
    };
    for (const workspaceId of ids) {
      if (!known.current.has(workspaceId)) check(workspaceId).catch(reportError);
    }
    return onAnyWorkspaceChange((workspaceId, change) => {
      if (workspaceId === null || change.kind !== "members" || !ids.includes(workspaceId)) return;
      check(workspaceId).catch(reportError);
    });
  }, [idList]);
}
