import { type ReactNode, useState } from "react";
import type { FileInfo } from "../../lib/api";
import type { LoadedImage } from "../../lib/files";
import { mayPreviewAsText } from "../../lib/textPreview";
import { cx } from "../../ui";
import { ImageViewer, TextViewer } from "./FileViewer";

/** A file's summary that opens the file in the viewer on a click, when the app can show it. */
export function OpensInViewer({
  file,
  image,
  className,
  children,
}: {
  file: FileInfo;
  image: LoadedImage;
  className: string;
  children: ReactNode;
}) {
  const [isOpen, setOpen] = useState(false);
  const isImage = image.status === "shown";
  if (!isImage && !mayPreviewAsText(file)) return <div className={className}>{children}</div>;
  return (
    <>
      <button
        type="button"
        aria-label={`View ${file.name}`}
        onClick={() => setOpen(true)}
        className={cx(className, "rounded-md text-left", isImage ? "cursor-zoom-in" : "cursor-pointer")}
      >
        {children}
      </button>
      {image.status === "shown" ? (
        <ImageViewer file={file} url={image.url} open={isOpen} onOpenChange={setOpen} />
      ) : (
        <TextViewer file={file} open={isOpen} onOpenChange={setOpen} />
      )}
    </>
  );
}
