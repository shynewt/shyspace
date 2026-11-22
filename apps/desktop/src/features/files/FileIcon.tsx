import { File, FileArchive, FileImage, FileText, FileVideo } from "lucide-react";

/** An icon for a file's kind, by its media type (only a hint). */
export function FileIcon({ mediaType }: { mediaType: string }) {
  if (mediaType.startsWith("image/")) return <FileImage size={18} />;
  if (mediaType.startsWith("video/")) return <FileVideo size={18} />;
  if (mediaType.startsWith("text/") || mediaType === "application/pdf") return <FileText size={18} />;
  if (mediaType === "application/zip") return <FileArchive size={18} />;
  return <File size={18} />;
}
