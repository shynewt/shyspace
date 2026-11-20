import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { showToast } from "../../lib/toast";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, Field, FormError, Input } from "../../ui";

export function ProfileSettings() {
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
      <p className="mt-2 text-[12px] leading-relaxed text-muted">
        Your status and appearing offline are in the menu under your name, at the bottom of the sidebar.
      </p>
    </form>
  );
}
