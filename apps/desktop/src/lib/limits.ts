/** Must match `MAX_MESSAGE_CHARS`, `MAX_PAGE_TITLE_CHARS` and `MAX_PROFILE_NAME_CHARS` in shy-core's ops.rs. */
export const MAX_MESSAGE_CHARS = 40_000;
export const MAX_PAGE_TITLE_CHARS = 200;
export const MAX_PROFILE_NAME_CHARS = 80;
/** Must match the limits in shy-core's profile.rs. */
export const PROFILE_LIMITS = {
  title: 80,
  pronouns: 40,
  bio: 300,
  email: 254,
  phone: 32,
  website: 200,
  location: 80,
} as const;
/** Where the composer starts showing how close a message is to the limit. */
export const SHOW_COUNT_FROM_CHARS = 36_000;

/** Characters as the core counts them (code points, not UTF-16 units). */
export function characterCount(text: string): number {
  return Array.from(text).length;
}

/** `text` cut to `max` characters as the core counts them. */
export function cutToCharacters(text: string, max: number): string {
  return Array.from(text).slice(0, max).join("");
}

/** Must match `MAX_FILE_BYTES` and `MAX_PICTURE_BYTES` in shy-core's files module, and `MAX_FILES_PER_MESSAGE` in workspace/files.rs. */
export const MAX_FILE_BYTES = 150 * 1024 * 1024;
export const MAX_PICTURE_BYTES = 512 * 1024;
export const MAX_FILES_PER_MESSAGE = 10;
/** Must match `MAX_PASTED_BYTES` in src-tauri/src/files.rs: pasted files cross the bridge as bytes. */
export const MAX_PASTED_BYTES = 25 * 1024 * 1024;
