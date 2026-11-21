import { LifeBuoy } from "lucide-react";
import { type FormEvent, useRef, useState } from "react";
import { api, type Session } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Field, TextArea } from "../../ui";
import { SubmitButton } from "./SubmitButton";

export function RestoreForm({ onReady }: { onReady: (session: Session) => void }) {
  const [kit, setKit] = useState("");
  const [isBusy, setBusy] = useState(false);
  const firstInput = useRef<HTMLTextAreaElement>(null);
  useFocusOnMount(firstInput);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      onReady(await api.restoreWorkspace(kit));
    } catch (error) {
      reportError(error);
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-3">
      <p className="flex gap-2 text-sm leading-relaxed text-muted">
        <LifeBuoy size={16} className="mt-0.5 shrink-0" />
        For a workspace admin whose device was lost. Only restore if the old device is gone: two admins at once would
        conflict.
      </p>
      <Field label="Recovery kit">
        {(id) => (
          <TextArea
            id={id}
            ref={firstInput}
            value={kit}
            onChange={(event) => setKit(event.target.value)}
            placeholder="shyrec1_…"
            rows={4}
            className="font-mono text-xs"
          />
        )}
      </Field>
      <SubmitButton isBusy={isBusy} isDisabled={!kit.trim().startsWith("shyrec1_")} icon={<LifeBuoy size={15} />}>
        {isBusy ? "Restoring…" : "Restore admin"}
      </SubmitButton>
    </form>
  );
}
