import { useCallback, useEffect, useState } from "react";
import { reportError } from "./toast";

/**
 * Per-device conveniences (not workspace data): kept in the webview's local
 * storage. Reads fall back to the default when storage is unavailable.
 */
export type Theme = "system" | "light" | "dark";
export const ACCENTS = ["violet", "blue", "indigo", "teal", "pink", "graphite"] as const;
export type Accent = (typeof ACCENTS)[number];
/** "system" follows the operating system's reduce-motion setting. */
export type ReduceMotion = "system" | "on" | "off";

export type RecentEmoji = { emoji: string; label: string };

type Preferences = {
  theme: Theme;
  accent: Accent;
  reduceMotion: ReduceMotion;
  /** Show message text in desktop notifications (off hides it). */
  alertsShowText: boolean;
  /** Newest first, at most RECENT_EMOJI_LIMIT. */
  recentEmoji: RecentEmoji[];
  /** Hides the line in Settings › Profile suggesting what to add. */
  isProfileNudgeDismissed: boolean;
  /** Hides the docked sidebar; a narrow window's drawer ignores it. */
  isSidebarHidden: boolean;
};

const DEFAULTS: Preferences = {
  theme: "system",
  accent: "violet",
  reduceMotion: "system",
  alertsShowText: true,
  recentEmoji: [],
  isProfileNudgeDismissed: false,
  isSidebarHidden: false,
};
const RECENT_EMOJI_LIMIT = 16;
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

/** Applies the chosen accent colour to the page; Violet is the stylesheet's default. */
export function useAccent() {
  const [{ accent }] = usePreferences();
  useEffect(() => {
    if (accent === "violet") document.documentElement.removeAttribute("data-accent");
    else document.documentElement.setAttribute("data-accent", accent);
  }, [accent]);
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

/** The recent emoji list after picking `picked`: it moves to the front, once. */
export function withRecentEmoji(recent: RecentEmoji[], picked: RecentEmoji): RecentEmoji[] {
  return [picked, ...recent.filter((entry) => entry.emoji !== picked.emoji)].slice(0, RECENT_EMOJI_LIMIT);
}

/** Stored recent emoji, skipping anything that isn't one (local storage is only ever a convenience). */
export function validRecentEmoji(stored: unknown): RecentEmoji[] {
  if (!Array.isArray(stored)) return [];
  return stored.flatMap((entry: unknown) =>
    typeof entry === "object" &&
    entry !== null &&
    "emoji" in entry &&
    "label" in entry &&
    typeof entry.emoji === "string" &&
    typeof entry.label === "string"
      ? [{ emoji: entry.emoji, label: entry.label }]
      : [],
  );
}
