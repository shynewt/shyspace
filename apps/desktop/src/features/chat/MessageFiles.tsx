import { Download, File, FileArchive, FileImage, FileText, FileVideo, Trash2 } from "lucide-react";
import { useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { formatSize, mayShowInline, useFileImage } from "../../lib/files";
import { reportError } from "../../lib/toast";
import { ConfirmDialog, IconButton } from "../../ui";

/** The files shared with a message: images inline, everything else as a card. */
export function MessageFiles({ files }: { files: FileInfo[] }) {
  if (files.length === 0) return null;
  return (
    <div className="mt-1 flex flex-col items-start gap-1.5">
      {files.map((file) =>
        mayShowInline(file) ? <InlineImage key={file.id} file={file} /> : <FileCard key={file.id} file={file} />,
      )}
    </div>
  );
}

function InlineImage({ file }: { file: FileInfo }) {
  const image = useFileImage(file);
  if (image.status === "not-an-image" || image.status === "failed") return <FileCard file={file} />;
  return (
    <figure className="group/file relative max-w-card overflow-hidden rounded-xl border border-line bg-surface-2">
      {image.status === "shown" ? (
        <img
          src={image.url}
          alt={file.name}
          draggable={false}
          className="block max-h-80 w-auto max-w-full object-contain"
        />
      ) : (
        <div role="img" aria-label={`Loading ${file.name}`} className="h-40 w-64" />
      )}
      <div className="absolute top-1.5 right-1.5 flex gap-0.5 rounded-lg border border-line bg-surface p-0.5 opacity-0 shadow-card transition group-hover/file:opacity-100 keyboard-focus-within:opacity-100">
        <FileActions file={file} />
      </div>
    </figure>
  );
}

function FileCard({ file }: { file: FileInfo }) {
  return (
    <div className="flex w-full max-w-card items-center gap-3 rounded-xl border border-line bg-surface px-3 py-2">
      <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-2 text-muted">
        <FileIcon mediaType={file.media_type} />
      </span>
      <div className="min-w-0 flex-1">
        <p className="selectable truncate text-sm font-medium">{file.name}</p>
        <p className="text-xs text-faint">{formatSize(file.size)}</p>
      </div>
      <FileActions file={file} />
    </div>
  );
}

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

export function FileIcon({ mediaType }: { mediaType: string }) {
  if (mediaType.startsWith("image/")) return <FileImage size={18} />;
  if (mediaType.startsWith("video/")) return <FileVideo size={18} />;
  if (mediaType.startsWith("text/") || mediaType === "application/pdf") return <FileText size={18} />;
  if (mediaType === "application/zip") return <FileArchive size={18} />;
  return <File size={18} />;
}
