import { Copy, Download, X } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { copyImage } from "../../lib/copyImage";
import { formatSize, type LoadedImage } from "../../lib/files";
import { ChatMarkdown } from "../../lib/markdown";
import { useMember } from "../../lib/members";
import { looksLikePdf, mayPreviewAsPdf } from "../../lib/pdfPreview";
import { displayName } from "../../lib/people";
import { decodeText, isMarkdownFile, mayPreviewAsText } from "../../lib/textPreview";
import { reportError, showToast } from "../../lib/toast";
import { linkedMessageView, useWorkspace, type View } from "../../lib/workspace";
import { Button, ContextMenu, Dialog, IconButton, type MenuAction, RelativeTime } from "../../ui";
import { ChannelName } from "../private/ChannelName";
import { FileActions } from "./FileActions";
import { FileIcon } from "./FileIcon";
import { PdfPages } from "./PdfPages";

type ViewerProps = {
  file: FileInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Off where the file is already shown in its message. */
  hasShowInChat: boolean;
};

/** A file shown large over the app, in the way that suits it: an image, a PDF's pages, text, or a note that it can't be shown. */
export function FilePreview({ image, ...props }: ViewerProps & { image: LoadedImage }) {
  if (image.status === "shown") return <ImageViewer {...props} url={image.url} />;
  if (mayPreviewAsPdf(props.file)) return <PdfViewer {...props} />;
  if (mayPreviewAsText(props.file)) return <TextViewer {...props} />;
  return <NoPreviewViewer {...props} />;
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

/** The viewer's frame: a bar saying what the file is, who shared it, where and when, with its actions; then its content. */
function FileViewer({ file, open, onOpenChange, hasShowInChat, children }: ViewerProps & { children: ReactNode }) {
  const sharer = useMember(file.author);
  const close = () => onOpenChange(false);
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={file.name}
      hideTitle
      width={1100}
      padding="none"
      initialFocus="dialog"
    >
      {/* The dialog itself scrolls (and has focus on opening, so the keys
          scroll it and the first Esc closes it); the bar stays on top. */}
      <div className="sticky top-0 z-10 flex items-center gap-3 border-b border-line bg-surface py-2 pr-2 pl-4">
        <div className="min-w-0 flex-1">
          <p className="selectable truncate text-sm font-medium">{file.name}</p>
          <p className="flex min-w-0 items-center gap-1 text-xs whitespace-nowrap text-faint">
            <span className="truncate">{displayName(sharer?.name ?? "", file.author)}</span>
            <span aria-hidden>·</span>
            <ChannelName channelId={file.place} className="min-w-0" />
            <span aria-hidden>·</span>
            <RelativeTime ms={file.created_ms} className="shrink-0" />
            <span aria-hidden>·</span>
            <span className="shrink-0">{formatSize(file.size)}</span>
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-0.5">
          <FileActions file={file} hasShowInChat={hasShowInChat} onLeave={close} />
          <div aria-hidden className="mx-1 h-5 w-px bg-line" />
          <IconButton label="Close" icon={<X size={16} />} onClick={close} />
        </div>
      </div>
      {children}
    </Dialog>
  );
}

function ImageViewer({ url, ...props }: ViewerProps & { url: string }) {
  const { file, onOpenChange } = props;
  return (
    <FileViewer {...props}>
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
function TextViewer(props: ViewerProps) {
  const { file, open, onOpenChange } = props;
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
    <FileViewer {...props}>
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
function PdfViewer(props: ViewerProps) {
  const { file, open } = props;
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
    <FileViewer {...props}>
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

/** A file the app can't show (an archive, a video): what it is, and a way to save it. */
function NoPreviewViewer(props: ViewerProps) {
  const { file } = props;
  return (
    <FileViewer {...props}>
      <div className="flex flex-col items-center gap-3 bg-surface-2 px-5 py-14 text-center">
        <span className="grid size-16 place-items-center rounded-2xl bg-surface text-muted shadow-card">
          <FileIcon name={file.name} mediaType={file.media_type} size={28} />
        </span>
        <p className="max-w-80 text-sm text-muted">
          There's no preview for this kind of file. Save it to open it with another app.
        </p>
        <Button
          variant="primary"
          icon={<Download size={15} />}
          onClick={() => {
            api.saveFile(file.id).catch(reportError);
          }}
        >
          Save
        </Button>
      </div>
    </FileViewer>
  );
}
