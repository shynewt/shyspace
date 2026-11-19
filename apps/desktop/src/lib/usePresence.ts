import { useEffect } from "react";
import { api } from "./api";
import { reportError } from "./toast";

const AWAY_AFTER_MS = 10 * 60 * 1000;
const ACTIVITY = ["pointermove", "keydown", "wheel", "focus"] as const;

/** Tells the core you're away after a while without input, and back on the next one. */
export function usePresence() {
  useEffect(() => {
    let isAway = false;
    let timer: ReturnType<typeof setTimeout>;
    const goAway = () => {
      isAway = true;
      api.setAway(true).catch(reportError);
    };
    const onActivity = () => {
      clearTimeout(timer);
      timer = setTimeout(goAway, AWAY_AFTER_MS);
      if (isAway) {
        isAway = false;
        api.setAway(false).catch(reportError);
      }
    };
    timer = setTimeout(goAway, AWAY_AFTER_MS);
    for (const event of ACTIVITY) window.addEventListener(event, onActivity, { passive: true });
    return () => {
      clearTimeout(timer);
      for (const event of ACTIVITY) window.removeEventListener(event, onActivity);
    };
  }, []);
}
