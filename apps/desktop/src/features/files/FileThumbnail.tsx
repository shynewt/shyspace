import type { LoadedImage } from "../../lib/files";
import { cx } from "../../ui";
import { FileIcon } from "./FileIcon";

/** A small square for a file: the image itself once it has loaded, its kind's icon otherwise. */
export function FileThumbnail({
  mediaType,
  image,
  size = "md",
}: {
  mediaType: string;
  image: LoadedImage;
  size?: "sm" | "md";
}) {
  const box = size === "sm" ? "size-8" : "size-9";
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
    <span className={cx(box, "grid shrink-0 place-items-center rounded-lg bg-surface-2 text-muted")}>
      <FileIcon mediaType={mediaType} />
    </span>
  );
}
