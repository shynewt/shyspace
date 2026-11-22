import { useEffect, useRef, useState } from "react";
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
  const [name, setName] = useState(file.name);
  const input = useRef<HTMLInputElement>(null);
  const save = useFormAction(async () => {
    if (name.trim() !== file.name) await api.renameFile(file.id, name);
    onOpenChange(false);
  });
  // Each opening starts from the current name (it may have changed elsewhere),
  // selected after the dialog has placed focus, so its focusing doesn't undo it.
  useEffect(() => {
    if (!open) return;
    setName(file.name);
    const frame = requestAnimationFrame(() => {
      const extension = extensionOf(file.name);
      const end = extension ? file.name.length - extension.length - 1 : file.name.length;
      input.current?.focus();
      input.current?.setSelectionRange(0, end);
    });
    return () => cancelAnimationFrame(frame);
  }, [open, file.name]);
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Rename file"
      description="Everyone who can see it sees the new name. The file itself doesn't change."
    >
      <form onSubmit={save.run} className="flex flex-col gap-4">
        <Field label="Name">
          {(id, description) => (
            <Input
              id={id}
              ref={input}
              {...description}
              value={name}
              maxLength={255}
              onChange={(event) => setName(event.target.value)}
            />
          )}
        </Field>
        <FormError message={save.error} />
        <DialogActions>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" isLoading={save.isBusy} disabled={!name.trim()}>
            Rename
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  );
}
