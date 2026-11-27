/** The sections of the Settings window, so any screen can open it at one. */
export type SettingsSection =
  | "general"
  | "profile"
  | "workspace"
  | "appearance"
  | "notifications"
  | "huddles"
  | "privacy"
  | "devices"
  | "members"
  | "agents"
  | "storage"
  | "recovery";

export type MembersTab = "members" | "invites" | "requests";

/** Fields in Settings › Profile that can be opened straight into. */
export type ProfileField = "name" | "title" | "pronouns" | "time_zone";

/**
 * Where Settings opens. `membersTab` picks a tab in Members (the admin's
 * only); `field` focuses a field in Profile.
 */
export type SettingsTarget = { section: SettingsSection; membersTab?: MembersTab; field?: ProfileField };

export type SettingsGroup = "you" | "workspace";

type SectionInfo = {
  id: SettingsSection;
  label: string;
  group: SettingsGroup;
  /** Only the admin can act here, so a member doesn't see it at all (the core refuses them anyway). */
  isAdminOnly?: boolean;
};

const SETTINGS_SECTIONS: SectionInfo[] = [
  { id: "general", label: "General", group: "you" },
  { id: "profile", label: "Profile", group: "you" },
  { id: "appearance", label: "Appearance", group: "you" },
  { id: "notifications", label: "Notifications", group: "you" },
  { id: "huddles", label: "Huddles", group: "you" },
  { id: "privacy", label: "Privacy & security", group: "you" },
  { id: "devices", label: "Devices", group: "you" },
  { id: "workspace", label: "Workspace", group: "workspace", isAdminOnly: true },
  { id: "members", label: "Members", group: "workspace" },
  { id: "agents", label: "Agents", group: "workspace", isAdminOnly: true },
  { id: "storage", label: "Storage", group: "workspace", isAdminOnly: true },
  { id: "recovery", label: "Recovery", group: "workspace", isAdminOnly: true },
];

/** The sections someone sees: all of them for the admin, a member's own and Members otherwise. */
export function sectionsShownTo(session: { is_admin: boolean }): SectionInfo[] {
  return SETTINGS_SECTIONS.filter((section) => session.is_admin || !section.isAdminOnly);
}
