import { useEffect, useSyncExternalStore } from "react";
import { api } from "./api";
import { CHECK_EVERY_MS, type RestartState, UpdateFlow } from "./updateFlow";

const flow = new UpdateFlow({
  check: api.checkForUpdate,
  restart: api.restartToUpdate,
  warn: (error) => console.error("checking for an update failed", error),
});

/** Looks for updates at launch and every few hours. Use once, at the app's root. */
export function useUpdateChecks() {
  useEffect(() => {
    void flow.checkNow();
    const timer = window.setInterval(() => void flow.checkNow(), CHECK_EVERY_MS);
    return () => window.clearInterval(timer);
  }, []);
}

/** The version downloaded and waiting for a restart, or null. */
export function useReadyUpdate(): string | null {
  return useSyncExternalStore(flow.subscribe, flow.readyVersion);
}

/** Where the app's one restart into the update stands. */
export function useRestartState(): RestartState {
  return useSyncExternalStore(flow.subscribe, flow.restartState);
}

/** Installs the ready update and restarts; refused during a huddle, and never started twice. */
export function restartToUpdate(): void {
  flow.restartToUpdate();
}
