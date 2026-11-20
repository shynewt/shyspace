/** The sections of the Settings window, so any screen can open it at one. */
export type SettingsSection = "profile" | "appearance" | "notifications" | "privacy" | "storage" | "recovery";

/** Where Settings opens. */
export type SettingsTarget = { section: SettingsSection };

export type SettingsGroup = "you" | "workspace";

export const SETTINGS_SECTIONS: { id: SettingsSection; label: string; group: SettingsGroup }[] = [
  { id: "profile", label: "Profile", group: "you" },
  { id: "appearance", label: "Appearance", group: "you" },
  { id: "notifications", label: "Notifications", group: "you" },
  { id: "privacy", label: "Privacy & security", group: "you" },
  { id: "storage", label: "Storage", group: "workspace" },
  { id: "recovery", label: "Recovery", group: "workspace" },
];
