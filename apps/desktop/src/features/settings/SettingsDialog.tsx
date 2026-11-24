import { Bell, Bot, HardDrive, Headphones, LifeBuoy, Palette, ShieldCheck, UserRound, Users } from "lucide-react";
import { motion } from "motion/react";
import { Tabs } from "radix-ui";
import { Fragment, type ReactNode } from "react";
import { type SettingsGroup, type SettingsSection, type SettingsTarget, sectionsShownTo } from "../../lib/settings";
import { useWorkspace } from "../../lib/workspace";
import { cx, Dialog, instantAfterAKey, springs } from "../../ui";
import { WaitingRequestsBadge } from "../members/JoinRequests";
import { AgentsSettings } from "./AgentsSettings";
import { AppearanceSettings } from "./AppearanceSettings";
import { HuddleSettings } from "./HuddleSettings";
import { MembersSettings } from "./MembersSettings";
import { NotificationSettings } from "./NotificationSettings";
import { PrivacySettings } from "./PrivacySettings";
import { ProfileSettings } from "./ProfileSettings";
import { RecoverySettings } from "./RecoverySettings";
import { StorageAttentionSign } from "./StorageProtection";
import { StorageSettings } from "./StorageSettings";
import { WorkspaceSettings } from "./WorkspaceSettings";

/** Profile and Members take a field or tab from the target, so they're rendered apart from these. */
const SECTION_VIEWS: Record<SettingsSection, { icon: ReactNode; description: string; content: ReactNode }> = {
  profile: {
    icon: <UserRound size={15} />,
    description: "How you appear to everyone in the workspace.",
    content: null,
  },
  appearance: {
    icon: <Palette size={15} />,
    description: "How shyspace looks on this device.",
    content: <AppearanceSettings />,
  },
  notifications: {
    icon: <Bell size={15} />,
    description: "When this device alerts you.",
    content: <NotificationSettings />,
  },
  huddles: {
    icon: <Headphones size={15} />,
    description: "Your microphone and speakers for voice huddles.",
    content: <HuddleSettings />,
  },
  privacy: {
    icon: <ShieldCheck size={15} />,
    description: "How your conversations are protected.",
    content: <PrivacySettings />,
  },
  workspace: {
    icon: <Palette size={15} />,
    description: "The workspace's icon and default look.",
    content: <WorkspaceSettings />,
  },
  members: {
    icon: <Users size={15} />,
    description: "Everyone who can read this workspace.",
    content: null,
  },
  agents: {
    icon: <Bot size={15} />,
    description: "AI helpers in this workspace, and how to add one.",
    content: <AgentsSettings />,
  },
  storage: {
    icon: <HardDrive size={15} />,
    description: "Where the workspace's encrypted data is kept.",
    content: <StorageSettings />,
  },
  recovery: {
    icon: <LifeBuoy size={15} />,
    description: "How the admin gets back in after losing their device.",
    content: <RecoverySettings />,
  },
};

/** Settings, at `target` while open. Choosing a section reports the new target. */
export function SettingsDialog({
  target,
  onNavigate,
  onClose,
}: {
  target: SettingsTarget | null;
  onNavigate: (target: SettingsTarget) => void;
  onClose: () => void;
}) {
  return (
    <Dialog
      open={target !== null}
      onOpenChange={(open) => !open && onClose()}
      title="Settings"
      width={760}
      padding="none"
      hasCloseButton
    >
      {target && <SettingsWindow target={target} onNavigate={onNavigate} />}
    </Dialog>
  );
}

function SettingsWindow({
  target,
  onNavigate,
}: {
  target: SettingsTarget;
  onNavigate: (target: SettingsTarget) => void;
}) {
  const { session } = useWorkspace();
  const membersTab = target.membersTab ?? "members";
  const sections = sectionsShownTo(session);
  // A section this person can't see (a stale link, say) opens at the first one instead.
  const shownSection = sections.some((section) => section.id === target.section) ? target.section : "profile";
  const groups: { id: SettingsGroup; title: string }[] = [
    { id: "you", title: "You" },
    { id: "workspace", title: session.workspace_name },
  ];
  const choose = (value: string) => {
    const section = sections.find((candidate) => candidate.id === value);
    if (section) onNavigate({ section: section.id });
  };
  return (
    <Tabs.Root
      value={shownSection}
      onValueChange={choose}
      orientation="vertical"
      className="@container/settings mt-4 flex h-settings max-h-[calc(100dvh-6rem)] border-t border-line"
    >
      <Tabs.List
        aria-label="Settings sections"
        className="flex w-45 shrink-0 flex-col @2xl/settings:w-49 gap-px overflow-y-auto border-r border-line bg-bg p-2"
      >
        {groups.map((group) => (
          <Fragment key={group.id}>
            <div
              aria-hidden
              className="truncate px-2.5 pt-3 pb-1 text-2xs font-semibold tracking-wide text-faint uppercase first:pt-1"
            >
              {group.title}
            </div>
            {sections
              .filter((section) => section.group === group.id)
              .map((section) => (
                <SectionTab
                  key={section.id}
                  section={section.id}
                  label={section.label}
                  isActive={section.id === shownSection}
                  badge={
                    (section.id === "members" && session.is_admin && <WaitingRequestsBadge />) ||
                    (section.id === "storage" && <StorageAttentionSign />)
                  }
                />
              ))}
          </Fragment>
        ))}
      </Tabs.List>
      {sections.map((section) => (
        // Every section stays mounted while Settings is open, so typed text and
        // a save still running survive looking at another section.
        <Tabs.Content
          key={section.id}
          value={section.id}
          forceMount
          tabIndex={-1}
          className="relative min-w-0 flex-1 overflow-y-auto px-6 pt-5 pb-6 outline-none data-[state=inactive]:hidden"
        >
          <header className="mb-5">
            <h3 className="text-lg font-semibold">{section.label}</h3>
            <p className="mt-0.5 text-sm text-muted">{SECTION_VIEWS[section.id].description}</p>
          </header>
          {section.id === "members" && (
            <MembersSettings
              tab={membersTab}
              onTabChange={(tab) => onNavigate({ section: "members", membersTab: tab })}
            />
          )}
          {section.id === "profile" && <ProfileSettings target={target} />}
          {SECTION_VIEWS[section.id].content}
        </Tabs.Content>
      ))}
    </Tabs.Root>
  );
}

function SectionTab({
  section,
  label,
  isActive,
  badge,
}: {
  section: SettingsSection;
  label: string;
  isActive: boolean;
  badge?: ReactNode;
}) {
  return (
    <Tabs.Trigger
      value={section}
      className={cx(
        "relative flex h-8 shrink-0 items-center gap-2.5 rounded-lg px-2.5 text-left text-sm transition-colors focus-ring-inset",
        isActive ? "font-medium text-ink" : "text-muted hover:bg-surface-2 hover:text-ink",
      )}
    >
      {isActive && (
        <motion.span
          layoutId="settings-section"
          transition={instantAfterAKey(springs.snappy)}
          className="absolute inset-0 rounded-lg bg-surface-3"
        >
          <span className="absolute inset-y-1.5 left-0 w-[3px] rounded-full bg-accent" />
        </motion.span>
      )}
      <span className={cx("relative", isActive ? "text-accent" : "text-decorative")}>
        {SECTION_VIEWS[section].icon}
      </span>
      <span className="relative truncate">{label}</span>
      {badge}
    </Tabs.Trigger>
  );
}
