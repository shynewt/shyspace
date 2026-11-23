import { type FocusEvent, type RefObject, useRef, useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { extensionOf } from "../../lib/fileKinds";
import { useFormAction } from "../../lib/useFormAction";
import { Button, Dialog, DialogActions, Field, FormError, Input } from "../../ui";

/** Asks for a file's new name, with the part before the extension selected. */
export function RenameFileDialog({
  file,
  open,
  onOpenChange,
}: {
  file: FileInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const field = useRef<HTMLInputElement>(null);
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      initialFocus={field}
      title="Rename file"
      description="Everyone who can see it sees the new name. The file itself doesn't change."
    >
      {/* Mounted for each opening, so it starts from the current name (it may have changed elsewhere). */}
      <RenameForm file={file} field={field} onDone={() => onOpenChange(false)} />
    </Dialog>
  );
}

function RenameForm({
  file,
  field,
  onDone,
}: {
  file: FileInfo;
  field: RefObject<HTMLInputElement | null>;
  onDone: () => void;
}) {
  const [name, setName] = useState(file.name);
  const hasSelected = useRef(false);
  const save = useFormAction(async () => {
    if (name.trim() !== file.name) await api.renameFile(file.id, name);
    onDone();
  });
  // Selected as the dialog focuses the field, so typing straight away replaces only the name, never the extension.
  const selectName = (event: FocusEvent<HTMLInputElement>) => {
    if (hasSelected.current) return;
    hasSelected.current = true;
    const extension = extensionOf(file.name);
    event.currentTarget.setSelectionRange(0, extension ? file.name.length - extension.length - 1 : file.name.length);
  };
  return (
    <form onSubmit={save.run} className="flex flex-col gap-4">
      <Field label="Name">
        {(id, description) => (
          <Input
            id={id}
            ref={field}
            {...description}
            value={name}
            maxLength={255}
            onFocus={selectName}
            onChange={(event) => setName(event.target.value)}
          />
        )}
      </Field>
      <FormError message={save.error} />
      <DialogActions>
        <Button variant="secondary" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" isLoading={save.isBusy} disabled={!name.trim()}>
          Rename
        </Button>
      </DialogActions>
    </form>
  );
}
