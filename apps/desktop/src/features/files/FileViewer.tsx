import { Copy, Download } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { copyImage } from "../../lib/copyImage";
import { formatSize } from "../../lib/files";
import { ChatMarkdown } from "../../lib/markdown";
import { looksLikePdf } from "../../lib/pdfPreview";
import { decodeText, isMarkdownFile } from "../../lib/textPreview";
import { reportError, showToast } from "../../lib/toast";
import { linkedMessageView, useWorkspace, type View } from "../../lib/workspace";
import { ContextMenu, Dialog, type MenuAction } from "../../ui";
import { FileActions } from "./FileActions";
import { PdfPages } from "./PdfPages";

/** A file shown large over the app: its name, size and actions, then its content. */
function FileViewer({
  file,
  open,
  onOpenChange,
  children,
}: {
  file: FileInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: React.ReactNode;
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
      initialFocus="dialog"
    >
      {/* The dialog itself scrolls (and has focus on opening, so the keys
          scroll it); the bar stays on top. */}
      <div className="sticky top-0 z-10 flex items-center gap-2 border-b border-line bg-surface py-2.5 pr-14 pl-4">
        <p className="selectable min-w-0 flex-1 truncate text-sm font-medium">{file.name}</p>
        <span className="shrink-0 text-xs text-faint">{formatSize(file.size)}</span>
        <FileActions file={file} />
      </div>
      {children}
    </Dialog>
  );
}

/** Right-click actions for a shown image. */
export function imageMenu(file: FileInfo, url: string): MenuAction[] {
  return [
    {
      label: "Copy image",
      icon: <Copy size={15} />,
      onSelect: () => {
        copyImage(url).then(
          () => showToast("Image copied."),
          (error) => reportError(error),
        );
      },
    },
    {
      label: "Save image…",
      icon: <Download size={15} />,
      onSelect: () => {
        api.saveFile(file.id).catch(reportError);
      },
    },
  ];
}

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
    <FileViewer file={file} open={open} onOpenChange={onOpenChange}>
      <ContextMenu items={imageMenu(file, url)}>
        <button
          type="button"
          aria-label="Close the viewer"
          onClick={() => onOpenChange(false)}
          className="grid w-full cursor-zoom-out place-items-center bg-surface-2 p-3"
        >
          <img src={url} alt={file.name} draggable={false} className="max-h-[75dvh] max-w-full object-contain" />
        </button>
      </ContextMenu>
    </FileViewer>
  );
}

type LoadedText = { status: "loading" } | { status: "shown"; text: string } | { status: "not-text" | "failed" };

/** A text file, read when opened: markdown formatted (never as HTML), anything else as plain text. */
export function TextViewer({
  file,
  open,
  onOpenChange,
}: {
  file: FileInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [loaded, setLoaded] = useState<LoadedText>({ status: "loading" });
  const { session, pages, navigate } = useWorkspace();
  const openElsewhere = (view: View | null) => {
    if (!view) return;
    onOpenChange(false);
    navigate(view);
  };
  useEffect(() => {
    if (!open) return;
    let isCurrent = true;
    setLoaded({ status: "loading" });
    api.fileBytes(file.id).then(
      (bytes) => {
        if (!isCurrent) return;
        const text = decodeText(bytes);
        setLoaded(text === null ? { status: "not-text" } : { status: "shown", text });
      },
      () => isCurrent && setLoaded({ status: "failed" }),
    );
    return () => {
      isCurrent = false;
    };
  }, [open, file.id]);

  return (
    <FileViewer file={file} open={open} onOpenChange={onOpenChange}>
      <div className="bg-bg px-5 py-4">
        {loaded.status === "loading" && <p className="text-sm text-muted">Opening…</p>}
        {loaded.status === "failed" && <p className="text-sm text-danger-text">This file couldn't be opened.</p>}
        {loaded.status === "not-text" && (
          <p className="text-sm text-muted">This isn't readable as text. Save it to open it with another app.</p>
        )}
        {loaded.status === "shown" &&
          (isMarkdownFile(file) ? (
            <div className="prose-chat prose-document selectable mx-auto max-w-reading text-base">
              <ChatMarkdown
                text={loaded.text}
                myId={session.me}
                pageTitle={(id) => pages.find((page) => page.id === id)?.title || undefined}
                onOpenPage={(id) => openElsewhere({ kind: "page", id })}
                onOpenMessage={(id) => {
                  api
                    .linkedMessage(id)
                    .then((linked) => openElsewhere(linked && linkedMessageView(linked)), reportError);
                }}
              />
            </div>
          ) : (
            <pre className="selectable font-mono text-sm leading-relaxed break-words whitespace-pre-wrap">
              {loaded.text}
            </pre>
          ))}
      </div>
    </FileViewer>
  );
}

type LoadedPdf = { status: "loading" } | { status: "shown"; bytes: Uint8Array } | { status: "not-pdf" | "failed" };

/** A PDF, read when opened and drawn page by page. */
export function PdfViewer({
  file,
  open,
  onOpenChange,
}: {
  file: FileInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [loaded, setLoaded] = useState<LoadedPdf>({ status: "loading" });
  useEffect(() => {
    if (!open) return;
    let isCurrent = true;
    setLoaded({ status: "loading" });
    api.fileBytes(file.id).then(
      (bytes) => isCurrent && setLoaded(looksLikePdf(bytes) ? { status: "shown", bytes } : { status: "not-pdf" }),
      () => isCurrent && setLoaded({ status: "failed" }),
    );
    return () => {
      isCurrent = false;
    };
  }, [open, file.id]);

  return (
    <FileViewer file={file} open={open} onOpenChange={onOpenChange}>
      <div className="bg-surface-2 p-4">
        {loaded.status === "loading" && <p className="text-sm text-muted">Opening…</p>}
        {loaded.status === "failed" && <p className="text-sm text-danger-text">This file couldn't be opened.</p>}
        {loaded.status === "not-pdf" && (
          <p className="text-sm text-muted">This isn't a PDF. Save it to open it with another app.</p>
        )}
        {loaded.status === "shown" && <PdfPages bytes={loaded.bytes} name={file.name} />}
      </div>
    </FileViewer>
  );
}
