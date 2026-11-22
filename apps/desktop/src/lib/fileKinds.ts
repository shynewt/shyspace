/** What kind of file something is, for its icon and label. Pure; a hint only, never how the file is shown. */

export type FileKind =
  | "document"
  | "code"
  | "data"
  | "spreadsheet"
  | "slides"
  | "archive"
  | "audio"
  | "video"
  | "image"
  | "other";

const KIND_BY_EXTENSION: Record<string, FileKind> = {
  pdf: "document",
  doc: "document",
  docx: "document",
  odt: "document",
  rtf: "document",
  txt: "document",
  md: "document",
  markdown: "document",
  log: "document",
  js: "code",
  jsx: "code",
  ts: "code",
  tsx: "code",
  rs: "code",
  py: "code",
  go: "code",
  rb: "code",
  java: "code",
  kt: "code",
  swift: "code",
  c: "code",
  h: "code",
  cpp: "code",
  sh: "code",
  sql: "code",
  html: "code",
  css: "code",
  json: "data",
  yaml: "data",
  yml: "data",
  toml: "data",
  xml: "data",
  ini: "data",
  env: "data",
  csv: "spreadsheet",
  tsv: "spreadsheet",
  xls: "spreadsheet",
  xlsx: "spreadsheet",
  ods: "spreadsheet",
  ppt: "slides",
  pptx: "slides",
  odp: "slides",
  key: "slides",
  zip: "archive",
  gz: "archive",
  tgz: "archive",
  tar: "archive",
  "7z": "archive",
  rar: "archive",
  mp3: "audio",
  wav: "audio",
  m4a: "audio",
  ogg: "audio",
  flac: "audio",
  mp4: "video",
  mov: "video",
  webm: "video",
  mkv: "video",
  avi: "video",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  heic: "image",
  svg: "image",
};

/** The extension, if the name has a short one. */
export function extensionOf(name: string): string | null {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return null;
  const extension = name.slice(dot + 1).toLowerCase();
  return /^[a-z0-9]{1,8}$/.test(extension) ? extension : null;
}

export function fileKindOf(name: string, mediaType: string): FileKind {
  const extension = extensionOf(name);
  const byExtension = extension ? KIND_BY_EXTENSION[extension] : undefined;
  if (byExtension) return byExtension;
  const [type] = mediaType.split("/");
  if (type === "image" || type === "audio" || type === "video") return type;
  if (type === "text") return "document";
  return "other";
}

/** The label under a file's icon: its extension in capitals, at most four letters. */
export function fileLabelOf(name: string): string | null {
  const extension = extensionOf(name);
  return extension && extension.length <= 4 ? extension.toUpperCase() : null;
}
