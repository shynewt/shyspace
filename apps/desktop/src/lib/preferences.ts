import { useCallback, useEffect, useState } from "react";
import { reportError } from "./toast";

/**
 * Per-device conveniences (not workspace data): kept in the webview's local
 * storage. Reads fall back to the default when storage is unavailable.
 */
export type Theme = "system" | "light" | "dark";
/** "system" follows the operating system's reduce-motion setting. */
export type ReduceMotion = "system" | "on" | "off";

type Preferences = {
  theme: Theme;
  reduceMotion: ReduceMotion;
  /** Show message text in desktop notifications (off hides it). */
  alertsShowText: boolean;
};

const DEFAULTS: Preferences = { theme: "system", reduceMotion: "system", alertsShowText: true };
const KEY = "shyspace.preferences";
const listeners = new Set<() => void>();

export function readPreferences(): Preferences {
  // Unreadable storage means the defaults, not an error: these are conveniences.
  try {
    const stored = localStorage.getItem(KEY);
    return stored ? { ...DEFAULTS, ...(JSON.parse(stored) as Partial<Preferences>) } : DEFAULTS;
  } catch {
    return DEFAULTS;
  }
}

function writePreferences(preferences: Preferences) {
  try {
    localStorage.setItem(KEY, JSON.stringify(preferences));
  } catch (error) {
    reportError(new Error(`couldn't save this setting; it lasts until the app closes (${String(error)})`));
  }
  for (const notify of listeners) notify();
}

export function usePreferences(): [Preferences, (change: Partial<Preferences>) => void] {
  const [preferences, setPreferences] = useState(readPreferences);
  useEffect(() => {
    const refresh = () => setPreferences(readPreferences());
    listeners.add(refresh);
    return () => {
      listeners.delete(refresh);
    };
  }, []);
  const update = useCallback((change: Partial<Preferences>) => {
    const next = { ...readPreferences(), ...change };
    writePreferences(next);
    setPreferences(next);
  }, []);
  return [preferences, update];
}

/** Applies the chosen theme to the page. */
export function useTheme() {
  const [{ theme }] = usePreferences();
  useEffect(() => {
    if (theme === "system") document.documentElement.removeAttribute("data-theme");
    else document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);
}

const MOTION_CONFIG: Record<ReduceMotion, "user" | "always" | "never"> = {
  system: "user",
  on: "always",
  off: "never",
};

/** The chosen reduce-motion setting, in the form Motion's `MotionConfig` takes. */
export function useReducedMotionConfig(): "user" | "always" | "never" {
  const [{ reduceMotion }] = usePreferences();
  return MOTION_CONFIG[reduceMotion];
}
