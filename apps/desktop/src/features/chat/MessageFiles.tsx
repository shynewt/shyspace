import { useCallback, useRef, useState } from "react";
import type { FileInfo } from "../../lib/api";
import { formatSize, mayShowInline, useFileImage } from "../../lib/files";
import { useContextMenu } from "../../lib/useContextMenu";
import { ContextMenu, cx } from "../../ui";
import { FileActions } from "../files/FileActions";
import { FileThumbnail } from "../files/FileThumbnail";
import { FilePreview, imageMenu } from "../files/FileViewer";

/** Shown on hover or keyboard focus, and kept while their menu is open. */
const HOVER_ACTIONS = "opacity-0 transition-opacity group-hover/file:opacity-100 keyboard-focus-within:opacity-100";

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
  const [isMenuOpen, setMenuOpen] = useState(false);
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
          <FilePreview file={file} image={image} open={isViewing} onOpenChange={setViewing} hasShowInChat={false} />
        </>
      ) : (
        <div role="img" aria-label={`Loading ${file.name}`} className="h-40 w-64" />
      )}
      <div
        className={cx(
          "absolute top-1.5 right-1.5 flex gap-0.5 rounded-lg border border-line bg-surface p-0.5 shadow-card",
          HOVER_ACTIONS,
          isMenuOpen && "opacity-100",
        )}
      >
        <FileActions file={file} hasShowInChat={false} isMenuOpen={isMenuOpen} onMenuOpenChange={setMenuOpen} />
      </div>
    </figure>
  );
}

/** A file that isn't shown inline: its kind, name and size. A click opens it; right-click opens its menu. */
function FileCard({ file }: { file: FileInfo }) {
  const [isViewing, setViewing] = useState(false);
  const [isMenuOpen, setMenuOpen] = useState(false);
  const card = useRef<HTMLDivElement>(null);
  const openMenu = useCallback(() => setMenuOpen(true), []);
  useContextMenu(card, openMenu);
  return (
    <div
      ref={card}
      data-own-menu=""
      className={cx(
        "group/file flex w-full max-w-card items-center gap-1 rounded-xl border border-line bg-surface pr-2 transition-colors hover:bg-surface-2",
        isMenuOpen && "bg-surface-2",
      )}
    >
      <button
        type="button"
        aria-label={`View ${file.name}`}
        onClick={() => setViewing(true)}
        className="focus-ring-inset flex min-w-0 flex-1 items-center gap-3 rounded-xl px-3 py-2 text-left"
      >
        <FileThumbnail name={file.name} mediaType={file.media_type} image={{ status: "none" }} />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium">{file.name}</span>
          <span className="block text-xs text-faint">{formatSize(file.size)}</span>
        </span>
      </button>
      <div className={cx("flex shrink-0", HOVER_ACTIONS, isMenuOpen && "opacity-100")}>
        <FileActions file={file} hasShowInChat={false} isMenuOpen={isMenuOpen} onMenuOpenChange={setMenuOpen} />
      </div>
      <FilePreview
        file={file}
        image={{ status: "none" }}
        open={isViewing}
        onOpenChange={setViewing}
        hasShowInChat={false}
      />
    </div>
  );
}
