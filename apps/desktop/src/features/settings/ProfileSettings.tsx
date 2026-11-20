import { type FormEvent, useCallback, useEffect, useState } from "react";
import { api, type NotifyLevel } from "../../lib/api";
import { reportError, showToast } from "../../lib/toast";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, Field, FormError, Input, SegmentedControl } from "../../ui";

export function ProfileSettings() {
  return (
    <div className="flex flex-col gap-4">
      <NameForm />
      <StatusSettings />
      <VisibilitySettings />
    </div>
  );
}

function NameForm() {
  const { session } = useWorkspace();
  const loadMembers = useCallback(() => api.members(), []);
  const members = useLive(loadMembers, (change) => change.kind === "members");
  const savedName = members?.find((member) => member.id === session.me)?.name ?? "";
  const [name, setName] = useState(savedName);
  useEffect(() => setName(savedName), [savedName]);
  const save = useFormAction(async () => {
    await api.setProfile(name);
    showToast("Your name is updated for everyone.");
  });
  return (
    <form onSubmit={save.run} className="flex flex-col gap-3">
      <Field label="Display name" hint="Everyone in the workspace sees this name.">
        {(id) => <Input id={id} value={name} onChange={(event) => setName(event.target.value)} />}
      </Field>
      <FormError message={save.error} />
      <Button
        type="submit"
        variant="primary"
        isLoading={save.isBusy}
        disabled={!name.trim() || name === savedName}
        className="self-start"
      >
        Save profile
      </Button>
    </form>
  );
}

type ClearAfter = "never" | "hour" | "four_hours" | "today";

function clearTime(choice: ClearAfter, now: Date): number | null {
  const hour = 60 * 60 * 1000;
  if (choice === "hour") return now.getTime() + hour;
  if (choice === "four_hours") return now.getTime() + 4 * hour;
  if (choice === "today") {
    const endOfDay = new Date(now);
    endOfDay.setHours(23, 59, 59, 0);
    return endOfDay.getTime();
  }
  return null;
}

function StatusSettings() {
  const { session } = useWorkspace();
  const loadMembers = useCallback(() => api.members(), []);
  const members = useLive(loadMembers, (change) => change.kind === "members");
  const current = members?.find((member) => member.id === session.me)?.status ?? null;
  const [emoji, setEmoji] = useState("");
  const [text, setText] = useState("");
  const [clearAfter, setClearAfter] = useState<ClearAfter>("never");
  const [alerts, setAlerts] = useState<NotifyLevel>("all");
  useEffect(() => {
    setEmoji(current?.emoji ?? "");
    setText(current?.text ?? "");
    setAlerts(current?.alerts ?? "all");
  }, [current?.emoji, current?.text, current?.alerts]);
  const save = (event: FormEvent) => {
    event.preventDefault();
    api
      .setStatus(emoji, text, clearTime(clearAfter, new Date()), alerts)
      .then(() => showToast("Status set."), reportError);
  };
  return (
    <form onSubmit={save} aria-label="Your status" className="flex flex-col gap-3 border-t border-line pt-4">
      <div className="flex gap-2">
        <Field label="Emoji">
          {(id) => (
            <Input
              id={id}
              value={emoji}
              onChange={(event) => setEmoji(event.target.value)}
              placeholder="🎧"
              className="w-16"
            />
          )}
        </Field>
        <div className="flex-1">
          <Field label="Status">
            {(id) => (
              <Input
                id={id}
                value={text}
                onChange={(event) => setText(event.target.value)}
                placeholder="Deep work"
                maxLength={100}
              />
            )}
          </Field>
        </div>
      </div>
      <Field label="Clear after">
        {() => (
          <SegmentedControl<ClearAfter>
            label="Clear after"
            options={[
              { value: "never", label: "Never" },
              { value: "hour", label: "1 hour" },
              { value: "four_hours", label: "4 hours" },
              { value: "today", label: "Today" },
            ]}
            value={clearAfter}
            onChange={setClearAfter}
          />
        )}
      </Field>
      <Field
        label="Notifications while set"
        hint="Mentions only also lets through replies in threads you follow. A channel you muted stays muted."
      >
        {() => (
          <SegmentedControl<NotifyLevel>
            label="Notifications while set"
            options={[
              { value: "all", label: "As usual" },
              { value: "mentions", label: "Mentions only" },
              { value: "nothing", label: "None" },
            ]}
            value={alerts}
            onChange={setAlerts}
          />
        )}
      </Field>
      <div className="flex gap-2">
        <Button type="submit" variant="primary" disabled={!emoji.trim() && !text.trim() && alerts === "all"}>
          Set status
        </Button>
        {current && (
          <Button
            variant="ghost"
            onClick={() => api.clearStatus().then(() => showToast("Status cleared."), reportError)}
          >
            Clear status
          </Button>
        )}
      </div>
    </form>
  );
}

function VisibilitySettings() {
  const isOffline = useLive(api.appearOffline, (change) => change.kind === "members");
  return (
    <div className="border-t border-line pt-4">
      <Field
        label="Show me as"
        hint="Online and away come from whether you're using shyspace. Appearing offline sends nothing, from the moment the app starts."
      >
        {() => (
          <SegmentedControl<"auto" | "offline">
            label="Show me as"
            options={[
              { value: "auto", label: "Online when here" },
              { value: "offline", label: "Appear offline" },
            ]}
            value={isOffline ? "offline" : "auto"}
            onChange={(choice) => api.setAppearOffline(choice === "offline").catch(reportError)}
          />
        )}
      </Field>
    </div>
  );
}
