import { Lock } from "lucide-react";
import { useState } from "react";
import { api } from "../../lib/api";
import { useWorkspace } from "../../lib/workspace";
import { Button, Dialog, Field, FormError, Input } from "../../ui";
import { MemberPicker, toggled, useOtherMembers } from "./MemberPicker";

const MAX_DM_OTHERS = 8;

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Names a private channel and picks who's in it. Only they can read it. */
export function NewPrivateChannelDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="New private channel"
      description="Only the people you add can read it, not even the workspace admin. You can add more later."
      width={520}
    >
      <NewPrivateChannelForm onDone={() => onOpenChange(false)} />
    </Dialog>
  );
}

function NewPrivateChannelForm({ onDone }: { onDone: () => void }) {
  const { navigate } = useWorkspace();
  const members = useOtherMembers();
  const [name, setName] = useState("");
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [isCreating, setCreating] = useState(false);
  const create = async () => {
    setCreating(true);
    setError(null);
    try {
      const id = await api.createPrivateChannel(name, [...chosen]);
      onDone();
      navigate({ kind: "channel", id });
    } catch (failure) {
      setError(message(failure));
      setCreating(false);
    }
  };
  return (
    <form
      className="flex flex-col gap-4 p-5 pt-4"
      onSubmit={(event) => {
        event.preventDefault();
        create();
      }}
    >
      <Field label="Name">
        {(id) => (
          <Input
            id={id}
            value={name}
            autoFocus
            placeholder="plans"
            onChange={(event) => setName(event.target.value.toLowerCase().replace(/\s+/g, "-"))}
          />
        )}
      </Field>
      <MemberPicker
        label="People to add"
        members={members}
        chosen={chosen}
        onToggle={(member) => setChosen(toggled(chosen, member))}
      />
      <FormError message={error} />
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button
          type="submit"
          variant="primary"
          icon={<Lock size={14} />}
          isLoading={isCreating}
          disabled={!name.trim()}
        >
          Create private channel
        </Button>
      </div>
    </form>
  );
}

/** Picks who to message. The same people always get the same DM. */
export function NewDmDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="New direct message"
      description="Only the people in it can read it."
      width={480}
    >
      <NewDmForm onDone={() => onOpenChange(false)} />
    </Dialog>
  );
}

function NewDmForm({ onDone }: { onDone: () => void }) {
  const { navigate } = useWorkspace();
  const members = useOtherMembers();
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [isOpening, setOpening] = useState(false);
  const isTooMany = chosen.size > MAX_DM_OTHERS;
  const open = async () => {
    setOpening(true);
    setError(null);
    try {
      const id = await api.openDm([...chosen]);
      onDone();
      navigate({ kind: "channel", id });
    } catch (failure) {
      setError(message(failure));
      setOpening(false);
    }
  };
  return (
    <div className="flex flex-col gap-4 p-5 pt-4">
      <MemberPicker
        label="People to message"
        members={members}
        chosen={chosen}
        onToggle={(member) => setChosen(toggled(chosen, member))}
      />
      {isTooMany && <p className="text-xs text-warning-text">A DM can have up to {MAX_DM_OTHERS} other people.</p>}
      <FormError message={error} />
      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button variant="primary" isLoading={isOpening} disabled={chosen.size === 0 || isTooMany} onClick={open}>
          Open DM
        </Button>
      </div>
    </div>
  );
}
