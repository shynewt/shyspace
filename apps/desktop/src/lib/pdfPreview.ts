/** Which shared files open as a PDF in the app. Pure. */

import { extensionOf } from "./fileKinds";

/** PDFs up to this size open in the viewer; larger ones are saved instead. */
export const MAX_PDF_PREVIEW_BYTES = 25 * 1024 * 1024;
/** Pages drawn in the viewer; a longer document says to save it for the rest. */
export const MAX_PDF_PREVIEW_PAGES = 30;

export function mayPreviewAsPdf(file: { name: string; media_type: string; size: number }): boolean {
  const isPdf = file.media_type === "application/pdf" || extensionOf(file.name) === "pdf";
  return isPdf && file.size <= MAX_PDF_PREVIEW_BYTES;
}

/** Whether the bytes really are a PDF: its marker sits in the first kilobyte. */
export function looksLikePdf(bytes: Uint8Array): boolean {
  const start = new TextDecoder("latin1").decode(bytes.subarray(0, 1024));
  return start.includes("%PDF-");
}
