import { Bell, HardDrive, Palette, ShieldCheck, UserRound } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type FormEvent, type ReactNode, useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { type Theme, usePreferences } from "../../lib/preferences";
import { reportError, showToast } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, cx, Dialog, Field, Input, SegmentedControl, springs } from "../../ui";
import { Recovery } from "../members/Recovery";
import { Storage } from "../members/Storage";

type Tab = "profile" | "appearance" | "notifications" | "storage" | "security";

const TABS: { id: Tab; label: string; icon: ReactNode; isAdminOnly?: boolean }[] = [
  { id: "profile", label: "Profile", icon: <UserRound size={15} /> },
  { id: "appearance", label: "Appearance", icon: <Palette size={15} /> },
  { id: "notifications", label: "Notifications", icon: <Bell size={15} /> },
  { id: "storage", label: "Storage", icon: <HardDrive size={15} />, isAdminOnly: true },
  { id: "security", label: "Security", icon: <ShieldCheck size={15} /> },
];

export function SettingsDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const { session } = useWorkspace();
  const [tab, setTab] = useState<Tab>("profile");
  const tabs = TABS.filter((candidate) => !candidate.isAdminOnly || session.is_admin);
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Settings" width={720}>
      <div className="flex min-h-[380px] gap-4 p-5 pt-3">
        <div
          role="tablist"
          aria-label="Settings sections"
          aria-orientation="vertical"
          className="flex w-44 shrink-0 flex-col gap-0.5"
        >
          {tabs.map((candidate) => (
            <button
              key={candidate.id}
              type="button"
              role="tab"
              aria-selected={tab === candidate.id}
              onClick={() => setTab(candidate.id)}
              className={cx(
                "relative flex h-8 items-center gap-2 rounded-lg px-2.5 text-left text-[13px]",
                tab === candidate.id ? "font-medium text-ink" : "text-muted hover:bg-surface-2 hover:text-ink",
              )}
            >
              {tab === candidate.id && (
                <motion.span
                  layoutId="settings-tab"
                  transition={springs.snappy}
                  className="absolute inset-0 rounded-lg bg-surface-2"
                />
              )}
              <span className="relative">{candidate.icon}</span>
              <span className="relative">{candidate.label}</span>
            </button>
          ))}
        </div>
        <div role="tabpanel" className="min-w-0 flex-1">
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={tab}
              initial={{ opacity: 0, x: 8 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -8 }}
              transition={{ duration: 0.15 }}
              className="flex flex-col gap-4"
            >
              {tab === "profile" && <ProfileSettings />}
              {tab === "appearance" && <AppearanceSettings />}
              {tab === "notifications" && <NotificationSettings />}
              {tab === "storage" && <Storage />}
              {tab === "security" && <SecuritySettings />}
            </motion.div>
          </AnimatePresence>
        </div>
      </div>
    </Dialog>
  );
}

function ProfileSettings() {
  const { session } = useWorkspace();
  const loadMembers = useCallback(() => api.members(), []);
  const members = useLive(loadMembers, (change) => change.kind === "members");
  const myName = members?.find((member) => member.id === session.me)?.name ?? "";
  const [name, setName] = useState(myName);
  useEffect(() => setName(myName), [myName]);
  const save = (event: FormEvent) => {
    event.preventDefault();
    api.setProfile(name).then(() => showToast("Your name is updated for everyone."), reportError);
  };
  return (
    <form onSubmit={save} className="flex flex-col gap-3">
      <Field label="Display name" hint="Everyone in the workspace sees this name.">
        {(id) => <Input id={id} value={name} onChange={(event) => setName(event.target.value)} />}
      </Field>
      <Button type="submit" variant="primary" disabled={!name.trim() || name === myName} className="self-start">
        Save
      </Button>
    </form>
  );
}

function AppearanceSettings() {
  const [preferences, update] = usePreferences();
  return (
    <Field label="Theme" hint="System follows your computer's light or dark setting.">
      {() => (
        <SegmentedControl<Theme>
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
          value={preferences.theme}
          onChange={(theme) => update({ theme })}
        />
      )}
    </Field>
  );
}

function NotificationSettings() {
  const [preferences, update] = usePreferences();
  return (
    <>
      <Field
        label="Message text in notifications"
        hint="Notifications appear outside shyspace's encryption, in your system's notification center. Hide the text to show only who wrote and where."
      >
        {() => (
          <SegmentedControl<"show" | "hide">
            options={[
              { value: "show", label: "Show text" },
              { value: "hide", label: "Hide text" },
            ]}
            value={preferences.alertsShowText ? "show" : "hide"}
            onChange={(choice) => update({ alertsShowText: choice === "show" })}
          />
        )}
      </Field>
      <p className="text-[12.5px] leading-relaxed text-muted">
        Choose which messages notify you per channel, from a channel's menu: every message, mentions only, or nothing.
      </p>
    </>
  );
}

function SecuritySettings() {
  const { session } = useWorkspace();
  return (
    <>
      <div className="flex flex-col gap-2 text-[13px] leading-relaxed text-muted">
        <h3 className="text-[14px] font-semibold text-ink">How this workspace is protected</h3>
        <p>
          Everything you write is encrypted on this device before it leaves, with keys only current members have. The
          storage bucket and the network only ever see scrambled data.
        </p>
        <p>
          When someone is removed, new keys are made that they never get, so they can't read anything written after.
          They keep what they already synced.
        </p>
        <p>
          Your copy is encrypted on disk too, with a key kept in your system's keychain. AI agents are members like
          anyone else: what they read goes to their AI provider.
        </p>
      </div>
      {session.is_admin && <Recovery />}
    </>
  );
}
