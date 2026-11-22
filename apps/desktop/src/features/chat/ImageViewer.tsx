import type { FileInfo } from "../../lib/api";
import { formatSize } from "../../lib/files";
import { Dialog } from "../../ui";
import { FileActions } from "./MessageFiles";

/** An image shown large over the app, with its name and actions. */
export function ImageViewer({
  file,
  url,
  open,
  onOpenChange,
}: {
  file: FileInfo;
  url: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={file.name}
      hideTitle
      width={1100}
      padding="none"
      hasCloseButton
    >
      {/* The image comes first, so opening focuses it (a click on it closes
          the viewer) rather than a button whose tooltip would take the first
          Escape; the bar shows above it. */}
      <div className="flex flex-col">
        <button
          type="button"
          aria-label="Close the viewer"
          onClick={() => onOpenChange(false)}
          className="grid cursor-zoom-out place-items-center bg-surface-2 p-3"
        >
          <img src={url} alt={file.name} draggable={false} className="max-h-[75dvh] max-w-full object-contain" />
        </button>
        <div className="order-first flex items-center gap-2 border-b border-line py-2.5 pr-14 pl-4">
          <p className="selectable min-w-0 flex-1 truncate text-sm font-medium">{file.name}</p>
          <span className="shrink-0 text-xs text-faint">{formatSize(file.size)}</span>
          <FileActions file={file} />
        </div>
      </div>
    </Dialog>
  );
}
