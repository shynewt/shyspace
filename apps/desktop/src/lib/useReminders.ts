import { useEffect } from "react";
import { api, type LaterItem, onChange } from "./api";
import { plainPreview } from "./markdown";
import { reportError, showReminderToast } from "./toast";

/** Checks at least this often, in case the clock jumps (sleep, time zones). */
const LONGEST_WAIT_MS = 30 * 1000;
const PREVIEW_CHARS = 140;

/**
 * Fires reminders right when they come due while the app runs; ones missed
 * while it was closed fire at launch.
 */
export function useReminders(onOpen: (item: LaterItem) => void) {
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let isStopped = false;

    const fireDue = async () => {
      for (const item of await api.takeDueReminders()) {
        const text = item.message?.message.text;
        const what = text ? plainPreview(text, () => undefined) : item.page?.title || "something you saved";
        const body = what.length > PREVIEW_CHARS ? `${what.slice(0, PREVIEW_CHARS - 1)}…` : what;
        showReminderToast(`Reminder: ${body}`, () => onOpen(item));
        api.showAlert("Reminder", body, item.message?.message.id ?? null).catch(reportError);
      }
    };

    const schedule = async () => {
      clearTimeout(timer);
      if (isStopped) return;
      await fireDue();
      const now = Date.now();
      const upcoming = (await api.later())
        .filter((item) => !item.is_done && item.remind_ms !== null && item.remind_ms > now)
        .map((item) => item.remind_ms as number);
      const wait = Math.min(LONGEST_WAIT_MS, ...upcoming.map((at) => at - now));
      if (!isStopped) timer = setTimeout(() => schedule().catch(reportError), Math.max(wait, 0));
    };

    schedule().catch(reportError);
    const unsubscribe = onChange((change) => {
      if (change.kind === "later") schedule().catch(reportError);
    });
    return () => {
      isStopped = true;
      clearTimeout(timer);
      unsubscribe();
    };
  }, [onOpen]);
}
