import { Bell, HardDrive, LifeBuoy, Palette, ShieldCheck, UserRound } from "lucide-react";
import { motion } from "motion/react";
import { Tabs } from "radix-ui";
import { Fragment, type ReactNode } from "react";
import { SETTINGS_SECTIONS, type SettingsGroup, type SettingsSection, type SettingsTarget } from "../../lib/settings";
import { useWorkspace } from "../../lib/workspace";
import { cx, Dialog, springs } from "../../ui";
import { AppearanceSettings } from "./AppearanceSettings";
import { NotificationSettings } from "./NotificationSettings";
import { PrivacySettings } from "./PrivacySettings";
import { ProfileSettings } from "./ProfileSettings";
import { RecoverySettings } from "./RecoverySettings";
import { StorageSettings } from "./StorageSettings";

const SECTION_VIEWS: Record<SettingsSection, { icon: ReactNode; description: string; content: ReactNode }> = {
  profile: {
    icon: <UserRound size={15} />,
    description: "How you appear to everyone in the workspace.",
    content: <ProfileSettings />,
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
  privacy: {
    icon: <ShieldCheck size={15} />,
    description: "How your conversations are protected.",
    content: <PrivacySettings />,
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
  const groups: { id: SettingsGroup; title: string }[] = [
    { id: "you", title: "You" },
    { id: "workspace", title: session.workspace_name },
  ];
  const choose = (value: string) => {
    const section = SETTINGS_SECTIONS.find((candidate) => candidate.id === value);
    if (section) onNavigate({ section: section.id });
  };
  return (
    <Tabs.Root
      value={target.section}
      onValueChange={choose}
      orientation="vertical"
      className="mt-4 flex h-[min(540px,calc(100dvh-6rem))] border-t border-line"
    >
      <Tabs.List
        aria-label="Settings sections"
        className="flex w-[196px] shrink-0 flex-col gap-px overflow-y-auto border-r border-line bg-bg p-2"
      >
        {groups.map((group) => (
          <Fragment key={group.id}>
            <div
              aria-hidden
              className="truncate px-2.5 pt-3 pb-1 text-[11px] font-semibold tracking-wide text-faint uppercase first:pt-1"
            >
              {group.title}
            </div>
            {SETTINGS_SECTIONS.filter((section) => section.group === group.id).map((section) => (
              <SectionTab
                key={section.id}
                section={section.id}
                label={section.label}
                isActive={section.id === target.section}
              />
            ))}
          </Fragment>
        ))}
      </Tabs.List>
      {SETTINGS_SECTIONS.map((section) => (
        <Tabs.Content
          key={section.id}
          value={section.id}
          className="min-w-0 flex-1 overflow-y-auto px-6 pt-5 pb-6 outline-none"
        >
          <header className="mb-5">
            <h3 className="text-[15px] font-semibold">{section.label}</h3>
            <p className="mt-0.5 text-[12.5px] text-muted">{SECTION_VIEWS[section.id].description}</p>
          </header>
          {SECTION_VIEWS[section.id].content}
        </Tabs.Content>
      ))}
    </Tabs.Root>
  );
}

function SectionTab({ section, label, isActive }: { section: SettingsSection; label: string; isActive: boolean }) {
  return (
    <Tabs.Trigger
      value={section}
      className={cx(
        "relative flex h-8 shrink-0 items-center gap-2.5 rounded-lg px-2.5 text-left text-[13px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset",
        isActive ? "font-medium text-ink" : "text-muted hover:bg-surface-2 hover:text-ink",
      )}
    >
      {isActive && (
        <motion.span
          layoutId="settings-section"
          transition={springs.snappy}
          className="absolute inset-0 rounded-lg bg-surface-3"
        >
          <span className="absolute inset-y-1.5 left-0 w-[3px] rounded-full bg-accent" />
        </motion.span>
      )}
      <span className={cx("relative", isActive ? "text-accent" : "text-faint")}>{SECTION_VIEWS[section].icon}</span>
      <span className="relative truncate">{label}</span>
    </Tabs.Trigger>
  );
}
