import {
  File,
  FileArchive,
  FileAudio,
  FileBraces,
  FileCode,
  FileImage,
  FileSpreadsheet,
  FileText,
  FileVideo,
  Presentation,
} from "lucide-react";
import { type FileKind, fileKindOf } from "../../lib/fileKinds";

const ICONS: Record<FileKind, typeof File> = {
  document: FileText,
  code: FileCode,
  data: FileBraces,
  spreadsheet: FileSpreadsheet,
  slides: Presentation,
  archive: FileArchive,
  audio: FileAudio,
  video: FileVideo,
  image: FileImage,
  other: File,
};

/** An icon for a file's kind, by its name and media type (only hints). */
export function FileIcon({ name, mediaType, size = 18 }: { name: string; mediaType: string; size?: number }) {
  const Icon = ICONS[fileKindOf(name, mediaType)];
  return <Icon size={size} />;
}
