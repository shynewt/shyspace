import { useEffect, useRef } from "react";
import { api, type LaterItem, onAnyWorkspaceChange } from "./api";
import { plainPreview } from "./plainPreview";
import { reportError, showReminderToast } from "./toast";

/** Checks at least this often, in case the clock jumps (sleep, time zones). */
const LONGEST_WAIT_MS = 30 * 1000;
const PREVIEW_CHARS = 140;

/**
 * Fires reminders right when they come due in any workspace on this device,
 * while the app runs; ones missed while it was closed fire at launch.
 * `onOpen` opens what was saved, in its own workspace.
 */
export function useReminders(onOpen: (workspaceId: string, item: LaterItem) => void) {
  const open = useRef(onOpen);
  useEffect(() => {
    open.current = onOpen;
  }, [onOpen]);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let isStopped = false;

    const fireDue = async () => {
      for (const { workspace_id: workspaceId, item } of await api.takeDueReminders()) {
        const text = item.message?.message.text;
        const what = text
          ? plainPreview(
              text,
              () => undefined,
              () => undefined,
              workspaceId,
            )
          : item.page?.title || "something you saved";
        const body = what.length > PREVIEW_CHARS ? `${what.slice(0, PREVIEW_CHARS - 1)}…` : what;
        showReminderToast(`Reminder: ${body}`, () => open.current(workspaceId, item));
        api
          .showAlert(
            "reminder",
            workspaceId,
            "Reminder",
            body,
            item.message ? { kind: "message", id: item.message.message.id } : null,
            null,
          )
          .catch(reportError);
      }
    };

    const schedule = async () => {
      clearTimeout(timer);
      if (isStopped) return;
      await fireDue();
      const next = await api.nextReminderMs();
      const wait = next === null ? LONGEST_WAIT_MS : Math.min(LONGEST_WAIT_MS, next - Date.now());
      if (!isStopped) timer = setTimeout(() => schedule().catch(reportError), Math.max(wait, 0));
    };

    schedule().catch(reportError);
    const unsubscribe = onAnyWorkspaceChange((_, change) => {
      if (change.kind === "later") schedule().catch(reportError);
    });
    return () => {
      isStopped = true;
      clearTimeout(timer);
      unsubscribe();
    };
  }, []);
}
