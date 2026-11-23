/**
 * Keeping the app current, without the UI: what's ready to install, and the
 * restart into it. The core side (`src-tauri/src/updates.rs`) downloads and
 * verifies updates, and refuses to restart during a huddle.
 */

export type RestartOutcome = "restarting" | "in_huddle" | "nothing_ready";

export type UpdateCalls = {
  /** The version downloaded and ready to install, or null while there's nothing newer. */
  check: () => Promise<string | null>;
  restart: () => Promise<RestartOutcome>;
  /** A check that failed: not the person's problem, so it's only logged. */
  warn: (error: unknown) => void;
};

/** How often a running app looks for an update, after the check at launch. */
export const CHECK_EVERY_MS = 4 * 60 * 60 * 1000;

export class UpdateFlow {
  private version: string | null = null;
  private readonly listeners = new Set<() => void>();
  private inFlight: Promise<void> | null = null;

  constructor(private readonly calls: UpdateCalls) {}

  /** The version waiting for a restart, if one is downloaded. */
  readonly readyVersion = (): string | null => this.version;

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  /** Looks for an update. Overlapping calls share one check; a failed one is tried again next time. */
  checkNow(): Promise<void> {
    this.inFlight ??= this.check().finally(() => {
      this.inFlight = null;
    });
    return this.inFlight;
  }

  restartToUpdate(): Promise<RestartOutcome> {
    return this.calls.restart();
  }

  private async check(): Promise<void> {
    try {
      const version = await this.calls.check();
      if (version === this.version) return;
      this.version = version;
      for (const listener of this.listeners) listener();
    } catch (error) {
      this.calls.warn(error);
    }
  }
}
