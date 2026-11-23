/**
 * An image in a page, drawn into its editor widget: the picture at its stored
 * width with resize handles, a caption under it, and the viewer on click.
 * Only files the core lists as this page's own come here with a `file`.
 */
import type { EditorView } from "@codemirror/view";
import { FileQuestion, ImageIcon } from "lucide-react";
import { motion } from "motion/react";
import { type KeyboardEvent, type PointerEvent, useEffect, useRef, useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { mayShowInline, useImage } from "../../lib/files";
import { reportError } from "../../lib/toast";
import { Button, cx, fade } from "../../ui";
import { FilePreview } from "../files/FileViewer";
import { imageAt, MAX_IMAGE_WIDTH, MIN_IMAGE_WIDTH, setImageCaption, setImageWidth } from "./editor/images";

export type PageImageProps = {
  view: EditorView;
  element: HTMLElement;
  pageId: string;
  fileId: string;
  /** Percent of the text column, or null for the image's own size. */
  width: number | null;
  caption: string;
  /** Undefined when the file isn't one of this page's (yet). */
  file: FileInfo | undefined;
  isSelected: boolean;
  isEditingCaption: boolean;
  onCaptionEditEnd: () => void;
};

export function PageImage(props: PageImageProps) {
  const { pageId, fileId, file } = props;
  const image = useImage(
    file && mayShowInline(file) ? { key: fileId, load: () => api.pageFileBytes(pageId, fileId) } : null,
  );
  const [isViewing, setViewing] = useState(false);
  if (!file || image.status === "none" || image.status === "not-an-image" || image.status === "failed") {
    return <Unavailable pageId={pageId} file={file} />;
  }
  return (
    <figure className="group/image my-2 flex flex-col items-center">
      <ResizableImage {...props} url={image.status === "shown" ? image.url : null} onOpen={() => setViewing(true)} />
      <Caption {...props} />
      <FilePreview file={file} image={image} open={isViewing} onOpenChange={setViewing} hasShowInPlace={false} />
    </figure>
  );
}

/** Where this widget's image sits in the document now. */
function rangeOf(view: EditorView, element: HTMLElement) {
  return imageAt(view.state, view.posAtDOM(element));
}

function ResizableImage({
  view,
  element,
  width,
  caption,
  file,
  isSelected,
  url,
  onOpen,
}: PageImageProps & { url: string | null; onOpen: () => void }) {
  const [dragWidth, setDragWidth] = useState<number | null>(null);
  const shownWidth = dragWidth ?? width;
  const name = caption || file?.name || "Image";

  const startResize = (event: PointerEvent<HTMLSpanElement>) => {
    event.preventDefault();
    const box = event.currentTarget.parentElement?.getBoundingClientRect();
    const column = view.contentDOM.getBoundingClientRect().width;
    if (!box || column <= 0) return;
    const center = box.left + box.width / 2;
    const widthAt = (x: number) =>
      Math.min(MAX_IMAGE_WIDTH, Math.max(MIN_IMAGE_WIDTH, Math.round(((Math.abs(x - center) * 2) / column) * 100)));
    let latest = widthAt(event.clientX);
    setDragWidth(latest);
    const move = (moved: globalThis.PointerEvent) => {
      latest = widthAt(moved.clientX);
      setDragWidth(latest);
    };
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      setDragWidth(null);
      const range = rangeOf(view, element);
      if (range) setImageWidth(view, range, latest);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  };

  return (
    <div
      className={cx("relative rounded-xl", isSelected && "ring-2 ring-accent")}
      style={{ width: shownWidth === null ? undefined : `${shownWidth}%`, maxWidth: "100%" }}
    >
      {url ? (
        <motion.img
          {...fade}
          src={url}
          alt={name}
          aria-description={`${shownWidth ?? MAX_IMAGE_WIDTH}% wide; Alt+arrows to resize`}
          draggable={false}
          onClick={onOpen}
          className="block w-full cursor-zoom-in rounded-xl"
        />
      ) : (
        <div
          role="img"
          aria-label={`${name}, loading`}
          className="grid aspect-[4/3] w-full place-items-center rounded-xl bg-surface-2"
          style={{ minWidth: shownWidth === null ? "16rem" : undefined }}
        >
          <ImageIcon size={22} className="text-decorative" aria-hidden />
        </div>
      )}
      {(["left", "right"] as const).map((side) => (
        <span
          key={side}
          aria-hidden
          data-side={side}
          onPointerDown={startResize}
          className={cx(
            "cm-image-handle absolute top-1/2 h-10 w-1.5 -translate-y-1/2 cursor-ew-resize rounded-full border border-line bg-surface shadow-card opacity-0 transition-opacity group-hover/image:opacity-100",
            side === "left" ? "left-1.5" : "right-1.5",
            (isSelected || dragWidth !== null) && "opacity-100",
          )}
        />
      ))}
      {dragWidth !== null && (
        <span className="absolute top-2 right-2 rounded-md bg-surface px-1.5 py-0.5 text-xs text-muted shadow-card">
          {dragWidth}%
        </span>
      )}
    </div>
  );
}

function Caption({ view, element, caption, isSelected, isEditingCaption, onCaptionEditEnd }: PageImageProps) {
  const [isEditing, setEditing] = useState(false);
  const [draft, setDraft] = useState(caption);
  const input = useRef<HTMLInputElement>(null);
  // Enter ends the edit, then the field's blur as it goes would end it again.
  const hasFinished = useRef(false);
  const editing = isEditing || isEditingCaption;

  useEffect(() => {
    if (!editing) return;
    hasFinished.current = false;
    setDraft(caption);
    input.current?.focus();
    input.current?.select();
  }, [editing, caption]);

  const finish = (save: boolean) => {
    if (hasFinished.current) return;
    hasFinished.current = true;
    setEditing(false);
    onCaptionEditEnd();
    const range = rangeOf(view, element);
    if (save && range && draft.trim() !== caption) setImageCaption(view, range, draft);
    view.focus();
  };
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    event.stopPropagation();
    if (event.key !== "Enter" && event.key !== "Escape") return;
    // Focus goes back to the editor now; the key's own action mustn't follow it there.
    event.preventDefault();
    finish(event.key === "Enter");
  };

  if (editing) {
    return (
      <input
        ref={input}
        aria-label="Caption"
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={onKeyDown}
        onBlur={() => finish(true)}
        className="mt-1.5 w-full max-w-reading bg-transparent text-center text-sm text-muted outline-none"
      />
    );
  }
  // Without a caption the image takes only its own height; selecting it offers one.
  if (!caption && !isSelected) return null;
  return (
    <button
      type="button"
      onClick={() => setEditing(true)}
      className={cx("mt-1.5 text-center text-sm", caption ? "text-muted" : "text-faint")}
    >
      {caption || "Add a caption"}
    </button>
  );
}

function Unavailable({ pageId, file }: { pageId: string; file: FileInfo | undefined }) {
  return (
    <span className="my-1 flex max-w-card items-center gap-3 rounded-xl border border-line px-3 py-2">
      <FileQuestion size={16} className="shrink-0 text-decorative" aria-hidden />
      <span className="min-w-0 flex-1 truncate text-sm text-muted">Image unavailable</span>
      {file && (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            api.savePageFile(pageId, file.id).catch(reportError);
          }}
        >
          Save
        </Button>
      )}
    </span>
  );
}
