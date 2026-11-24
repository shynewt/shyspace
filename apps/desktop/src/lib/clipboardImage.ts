import { api } from "./api";

/**
 * True for a paste that carried neither files nor text. Some WebKitGTK builds
 * hand the page such an empty paste when the clipboard holds an image.
 */
export function isEmptyPaste(clipboard: DataTransfer): boolean {
  return clipboard.files.length === 0 && clipboard.getData("text/plain") === "";
}

/** The system clipboard's image as a PNG named like a pasted screenshot. */
export async function clipboardImageFile(): Promise<File> {
  const framed = await api.clipboardImage();
  const header = new DataView(framed.buffer, framed.byteOffset, 8);
  const width = header.getUint32(0, true);
  const height = header.getUint32(4, true);
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("couldn't draw the clipboard's image");
  context.putImageData(new ImageData(new Uint8ClampedArray(framed.subarray(8)), width, height), 0, 0);
  const png = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!png) throw new Error("couldn't draw the clipboard's image");
  return new File([png], "image.png", { type: "image/png" });
}
