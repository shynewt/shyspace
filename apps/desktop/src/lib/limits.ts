/** Must match `MAX_MESSAGE_CHARS` and `MAX_PAGE_TITLE_CHARS` in shy-core's ops.rs. */
export const MAX_MESSAGE_CHARS = 40_000;
export const MAX_PAGE_TITLE_CHARS = 200;
/** Where the composer starts showing how close a message is to the limit. */
export const SHOW_COUNT_FROM_CHARS = 36_000;

/** Characters as the core counts them (code points, not UTF-16 units). */
export function characterCount(text: string): number {
  return Array.from(text).length;
}
