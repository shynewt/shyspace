import { Download, Trash2 } from "lucide-react";
import { useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { ConfirmDialog, IconButton } from "../../ui";

/** Save, and Delete for those who may. */
export function FileActions({ file }: { file: FileInfo }) {
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
  const save = async () => {
    try {
      await api.saveFile(file.id);
    } catch (error) {
      reportError(error);
    }
  };
  return (
    <>
      <IconButton label={`Save ${file.name}`} icon={<Download size={15} />} onClick={save} />
      {file.can_delete && (
        <IconButton
          label={`Delete ${file.name}`}
          icon={<Trash2 size={15} />}
          onClick={() => setConfirmingDelete(true)}
        />
      )}
      <ConfirmDialog
        open={isConfirmingDelete}
        onOpenChange={setConfirmingDelete}
        title="Delete this file?"
        description="It disappears for everyone and leaves the workspace's storage. Anyone who already saved a copy keeps it. This can't be undone."
        confirmLabel="Delete file"
        onConfirm={() => api.deleteFile(file.id)}
      >
        <p className="selectable truncate text-sm">{file.name}</p>
      </ConfirmDialog>
    </>
  );
}
