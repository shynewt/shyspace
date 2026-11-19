/**
 * Keyboard shortcuts, in one place: the shell handles them, the cheat sheet
 * lists them, and menus show them. Each uses a modifier, since the message
 * box usually has focus and plain letters belong to it.
 */

const isMac = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");
export const MOD_LABEL = isMac ? "⌘" : "Ctrl";

const hasMod = (event: KeyboardEvent) => (isMac ? event.metaKey : event.ctrlKey);

export type ShortcutId = "search" | "inbox" | "previous" | "next" | "previousUnread" | "nextUnread" | "cheatSheet";

export type Shortcut = {
  id: ShortcutId;
  keys: string[];
  description: string;
  matches: (event: KeyboardEvent) => boolean;
};

export const SHORTCUTS: Shortcut[] = [
  {
    id: "search",
    keys: [MOD_LABEL, "K"],
    description: "Search, jump to, or run an action",
    matches: (event) => hasMod(event) && !event.shiftKey && event.key.toLowerCase() === "k",
  },
  {
    id: "inbox",
    keys: [MOD_LABEL, "Shift", "A"],
    description: "Open the inbox",
    matches: (event) => hasMod(event) && event.shiftKey && event.key.toLowerCase() === "a",
  },
  {
    id: "previous",
    keys: ["Alt", "↑"],
    description: "Previous channel or page",
    matches: (event) => event.altKey && !event.shiftKey && event.key === "ArrowUp",
  },
  {
    id: "next",
    keys: ["Alt", "↓"],
    description: "Next channel or page",
    matches: (event) => event.altKey && !event.shiftKey && event.key === "ArrowDown",
  },
  {
    id: "previousUnread",
    keys: ["Alt", "Shift", "↑"],
    description: "Previous unread channel",
    matches: (event) => event.altKey && event.shiftKey && event.key === "ArrowUp",
  },
  {
    id: "nextUnread",
    keys: ["Alt", "Shift", "↓"],
    description: "Next unread channel",
    matches: (event) => event.altKey && event.shiftKey && event.key === "ArrowDown",
  },
  {
    id: "cheatSheet",
    keys: [MOD_LABEL, "/"],
    description: "Show keyboard shortcuts",
    matches: (event) => hasMod(event) && event.key === "/",
  },
];

/** Shown in the cheat sheet but handled where they apply. */
export const LOCAL_SHORTCUTS: { keys: string[]; description: string }[] = [
  { keys: ["Esc"], description: "Hide the new-messages line" },
  { keys: ["Enter"], description: "Send a message" },
  { keys: ["Shift", "Enter"], description: "New line in a message" },
  { keys: ["@"], description: "Mention someone" },
];

export function shortcutKeys(id: ShortcutId): string[] {
  const shortcut = SHORTCUTS.find((candidate) => candidate.id === id);
  if (!shortcut) throw new Error(`no shortcut ${id}`);
  return shortcut.keys;
}
