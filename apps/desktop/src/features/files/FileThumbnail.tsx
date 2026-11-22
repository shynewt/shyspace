import { fileLabelOf } from "../../lib/fileKinds";
import type { LoadedImage } from "../../lib/files";
import { cx } from "../../ui";
import { FileIcon } from "./FileIcon";

/**
 * A small square for a file: the image itself once it has loaded; otherwise
 * its kind's icon over its extension, so a PDF and a text file tell apart.
 */
export function FileThumbnail({
  name,
  mediaType,
  image,
  size = "md",
}: {
  name: string;
  mediaType: string;
  image: LoadedImage;
  size?: "sm" | "md";
}) {
  const box = size === "sm" ? "size-8" : "size-9";
  const label = fileLabelOf(name);
  if (image.status === "shown") {
    return (
      <img
        src={image.url}
        alt=""
        draggable={false}
        className={cx(box, "shrink-0 rounded-lg border border-line object-cover")}
      />
    );
  }
  return (
    <span
      className={cx(
        box,
        "flex shrink-0 flex-col items-center justify-center gap-px rounded-lg bg-surface-2 text-muted",
      )}
    >
      <FileIcon name={name} mediaType={mediaType} size={label ? 14 : 18} />
      {label && <span className="text-2xs leading-none font-semibold">{label}</span>}
    </span>
  );
}
