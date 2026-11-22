import { Upload as UploadIcon, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type { StagedFile } from "../../lib/api";
import { formatSize, useStagedImage } from "../../lib/files";
import { MAX_FILE_BYTES } from "../../lib/limits";
import { cx, fade, IconButton } from "../../ui";
import { FileThumbnail } from "../files/FileThumbnail";
import type { Upload } from "./useAttachments";

/** Files waiting in the message box, each removable. */
export function AttachmentList({
  files,
  problem,
  onRemove,
}: {
  files: StagedFile[];
  problem: string | null;
  onRemove: (token: string) => void;
}) {
  if (files.length === 0) return null;
  return (
    <div className="border-b border-line px-2 pt-2 pb-1.5">
      <ul aria-label="Files to send" className="flex flex-wrap gap-1.5">
        {files.map((file) => (
          <li
            key={file.token}
            className={cx(
              "flex max-w-60 items-center gap-2 rounded-lg border bg-surface-2 py-1 pr-1 pl-1",
              file.size > MAX_FILE_BYTES ? "border-danger/60" : "border-line",
            )}
          >
            <StagedThumbnail file={file} />
            <span className="min-w-0">
              <span className="block truncate text-sm">{file.name}</span>
              <span className="block text-2xs text-faint">{formatSize(file.size)}</span>
            </span>
            <IconButton
              label={`Remove ${file.name}`}
              size="sm"
              icon={<X size={13} />}
              onClick={() => onRemove(file.token)}
            />
          </li>
        ))}
      </ul>
      {problem && (
        <p role="alert" className="mt-1.5 px-0.5 text-xs text-danger-text">
          {problem}
        </p>
      )}
    </div>
  );
}

function StagedThumbnail({ file }: { file: StagedFile }) {
  return <FileThumbnail name={file.name} mediaType={file.media_type} image={useStagedImage(file)} size="sm" />;
}

/** How far the upload of the files being sent has got. */
export function UploadProgress({ upload }: { upload: Upload }) {
  const fraction = upload.total > 0 ? upload.sent / upload.total : 0;
  return (
    <div
      role="progressbar"
      aria-label="Uploading files"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(fraction * 100)}
      className="h-0.5 overflow-hidden bg-surface-2"
    >
      <div className="h-full origin-left bg-accent" style={{ transform: `scaleX(${fraction})` }} />
    </div>
  );
}

/** Shown over the message box while files are dragged over the window. */
export function DropHint({ isShown, label }: { isShown: boolean; label: string }) {
  return (
    <AnimatePresence>
      {isShown && (
        <motion.div
          {...fade}
          className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center gap-2 rounded-xl border-2 border-dashed border-accent bg-accent-soft text-sm font-medium text-accent-text"
        >
          <UploadIcon size={16} /> Drop to attach to “{label}”
        </motion.div>
      )}
    </AnimatePresence>
  );
}
