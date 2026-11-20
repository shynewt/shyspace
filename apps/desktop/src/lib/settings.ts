/** The sections of the Settings window, so any screen can open it at one. */
export type SettingsSection =
  | "profile"
  | "appearance"
  | "notifications"
  | "privacy"
  | "members"
  | "agents"
  | "storage"
  | "recovery";

export type MembersTab = "members" | "invites" | "requests";

/** Where Settings opens. `membersTab` picks a tab in Members (the admin's only). */
export type SettingsTarget = { section: SettingsSection; membersTab?: MembersTab };

export type SettingsGroup = "you" | "workspace";

export const SETTINGS_SECTIONS: { id: SettingsSection; label: string; group: SettingsGroup }[] = [
  { id: "profile", label: "Profile", group: "you" },
  { id: "appearance", label: "Appearance", group: "you" },
  { id: "notifications", label: "Notifications", group: "you" },
  { id: "privacy", label: "Privacy & security", group: "you" },
  { id: "members", label: "Members", group: "workspace" },
  { id: "agents", label: "Agents", group: "workspace" },
  { id: "storage", label: "Storage", group: "workspace" },
  { id: "recovery", label: "Recovery", group: "workspace" },
];
