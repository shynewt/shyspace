/** One of a page's files in the viewer, opened from its chip in the page. */
import type { FileInfo } from "../../lib/api";
import { api } from "../../lib/api";
import { mayShowInline, useImage } from "../../lib/files";
import { FilePreview } from "../files/FileViewer";

export function PageFileViewer({ pageId, file, onClose }: { pageId: string; file: FileInfo; onClose: () => void }) {
  const image = useImage(mayShowInline(file) ? { key: file.id, load: () => api.pageFileBytes(pageId, file.id) } : null);
  return (
    <FilePreview file={file} image={image} open onOpenChange={(open) => !open && onClose()} hasShowInPlace={false} />
  );
}
