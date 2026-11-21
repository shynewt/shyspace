/**
 * Keyboard shortcuts, in one place: the shell handles them, the cheat sheet
 * lists them, and menus show them. Each uses a modifier, since the message
 * box usually has focus and plain letters belong to it.
 */

const isMac = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");
export const MOD_LABEL = isMac ? "⌘" : "Ctrl";

/**
 * Whether Ctrl, Alt or Meta is held, which makes a key a shortcut rather than
 * movement. Super+arrows belong to the window manager, which on Wayland can
 * also hand the app the keys still held when its window gains focus.
 */
export function hasCommandModifier(event: { ctrlKey: boolean; altKey: boolean; metaKey: boolean }): boolean {
  return event.ctrlKey || event.altKey || event.metaKey;
}

const hasMod = (event: KeyboardEvent) => (isMac ? event.metaKey : event.ctrlKey);

/** Option+arrows move by word on a Mac, so back and forward use ⌘[ and ⌘] there, like a browser. */
const isBracketWithCommand = (event: KeyboardEvent, bracket: "[" | "]") =>
  event.metaKey && !event.altKey && !event.ctrlKey && !event.shiftKey && event.key === bracket;

const isAltArrow = (event: KeyboardEvent, arrow: "ArrowLeft" | "ArrowRight") =>
  event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey && event.key === arrow;

export type ShortcutId =
  | "search"
  | "inbox"
  | "back"
  | "forward"
  | "previous"
  | "next"
  | "previousUnread"
  | "nextUnread"
  | "cheatSheet"
  | "settings";

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
    id: "back",
    keys: isMac ? [MOD_LABEL, "["] : ["Alt", "←"],
    description: "Back to where you were",
    matches: (event) => (isMac ? isBracketWithCommand(event, "[") : isAltArrow(event, "ArrowLeft")),
  },
  {
    id: "forward",
    keys: isMac ? [MOD_LABEL, "]"] : ["Alt", "→"],
    description: "Forward again",
    matches: (event) => (isMac ? isBracketWithCommand(event, "]") : isAltArrow(event, "ArrowRight")),
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
    id: "settings",
    keys: [MOD_LABEL, ","],
    description: "Open settings",
    matches: (event) => hasMod(event) && event.key === ",",
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
  { keys: ["Esc"], description: "Mark the channel read and jump to the newest message" },
  { keys: ["Enter"], description: "Send a message" },
  { keys: ["Shift", "Enter"], description: "New line in a message" },
  { keys: ["@"], description: "Mention someone" },
  { keys: ["/"], description: "Add a heading, list, checklist or other block in a page" },
  { keys: ["[", "["], description: "Link to another page from a page" },
  { keys: [MOD_LABEL, "B"], description: "Bold text in a page" },
  { keys: [MOD_LABEL, "I"], description: "Italic text in a page" },
  { keys: [MOD_LABEL, "E"], description: "Code in a page" },
  { keys: [MOD_LABEL, "K"], description: "Link the selected text in a page" },
  { keys: [MOD_LABEL, "Enter"], description: "Check off an item, open a link, or show who a mention is, in a page" },
];

export function shortcutKeys(id: ShortcutId): string[] {
  const shortcut = SHORTCUTS.find((candidate) => candidate.id === id);
  if (!shortcut) throw new Error(`no shortcut ${id}`);
  return shortcut.keys;
}
