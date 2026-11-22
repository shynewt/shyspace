import { useState } from "react";
import type { FileInfo } from "../../lib/api";
import { formatSize, mayShowInline, useFileImage } from "../../lib/files";
import { ContextMenu } from "../../ui";
import { FileActions } from "../files/FileActions";
import { FileThumbnail } from "../files/FileThumbnail";
import { ImageViewer, imageMenu } from "../files/FileViewer";
import { OpensInViewer } from "../files/OpensInViewer";

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
  const [isViewing, setViewing] = useState(false);
  if (image.status === "not-an-image" || image.status === "failed") return <FileCard file={file} />;
  return (
    <figure className="group/file relative max-w-card overflow-hidden rounded-xl border border-line bg-surface-2">
      {image.status === "shown" ? (
        <>
          <ContextMenu items={imageMenu(file, image.url)}>
            <button
              type="button"
              aria-label={`View ${file.name}`}
              onClick={() => setViewing(true)}
              className="block cursor-zoom-in"
            >
              <img
                src={image.url}
                alt={file.name}
                draggable={false}
                className="block max-h-80 w-auto max-w-full object-contain"
              />
            </button>
          </ContextMenu>
          <ImageViewer file={file} url={image.url} open={isViewing} onOpenChange={setViewing} />
        </>
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
      <OpensInViewer file={file} image={{ status: "none" }} className="flex min-w-0 flex-1 items-center gap-3">
        <FileThumbnail mediaType={file.media_type} image={{ status: "none" }} />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium">{file.name}</span>
          <span className="block text-xs text-faint">{formatSize(file.size)}</span>
        </span>
      </OpensInViewer>
      <FileActions file={file} />
    </div>
  );
}
