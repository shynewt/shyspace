/** Which shared files can be read as text in the app, and how. Pure. */

import { extensionOf } from "./fileKinds";

/** Text files up to this size open in the viewer; larger ones are saved instead. */
export const MAX_TEXT_PREVIEW_BYTES = 1024 * 1024;

const TEXT_EXTENSIONS = new Set([
  "txt",
  "md",
  "markdown",
  "csv",
  "tsv",
  "log",
  "json",
  "yaml",
  "yml",
  "toml",
  "ini",
  "conf",
  "xml",
  "html",
  "css",
  "js",
  "jsx",
  "ts",
  "tsx",
  "rs",
  "py",
  "go",
  "rb",
  "java",
  "kt",
  "swift",
  "c",
  "h",
  "cpp",
  "sh",
  "sql",
  "env",
]);
const TEXT_MEDIA_TYPES = new Set(["application/json", "application/xml", "application/x-yaml", "application/toml"]);

/** Whether a file may be text worth opening; its bytes decide in the end. */
export function mayPreviewAsText(file: { name: string; media_type: string; size: number }): boolean {
  const isText =
    file.media_type.startsWith("text/") ||
    TEXT_MEDIA_TYPES.has(file.media_type) ||
    TEXT_EXTENSIONS.has(extensionOf(file.name) ?? "");
  return isText && file.size <= MAX_TEXT_PREVIEW_BYTES;
}

export function isMarkdownFile(file: { name: string; media_type: string }): boolean {
  return file.media_type === "text/markdown" || ["md", "markdown"].includes(extensionOf(file.name) ?? "");
}

/** The bytes as text, or null when they aren't valid UTF-8 text (a NUL byte means binary). */
export function decodeText(bytes: Uint8Array): string | null {
  if (bytes.includes(0)) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}
