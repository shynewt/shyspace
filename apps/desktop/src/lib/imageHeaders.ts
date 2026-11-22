/**
 * What an image's own header says it is, read before decoding: decoding is
 * what costs memory, and a few hundred kilobytes can claim gigapixels. When
 * a header is unusual, this refuses rather than guesses.
 */

const MAX_IMAGE_SIDE_PX = 8192;
const MAX_IMAGE_PIXELS = 40_000_000;

type ImageInfo = { type: string; width: number; height: number };

/**
 * What these bytes really are, if a PNG, JPEG, GIF or WebP image, with the
 * largest size its headers claim. Never SVG or anything else that can run code.
 */
export function imageInfoOf(bytes: Uint8Array): ImageInfo | null {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const has = (offset: number, prefix: number[]) => prefix.every((byte, index) => bytes[offset + index] === byte);
  if (bytes.length >= 24 && has(0, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) {
    return { type: "image/png", width: view.getUint32(16), height: view.getUint32(20) };
  }
  if (bytes.length >= 13 && has(0, [0x47, 0x49, 0x46, 0x38])) return gifInfo(bytes, view);
  if (bytes.length >= 30 && has(0, [0x52, 0x49, 0x46, 0x46]) && has(8, [0x57, 0x45, 0x42, 0x50])) {
    return webpInfo(bytes, view);
  }
  if (bytes.length >= 4 && has(0, [0xff, 0xd8])) return jpegInfo(bytes, view);
  return null;
}

/** The type to show these bytes as, or null when they aren't an image this app shows or are too big to decode. */
export function showableImageType(bytes: Uint8Array): string | null {
  const info = imageInfoOf(bytes);
  if (!info || info.width === 0 || info.height === 0) return null;
  const isTooBig =
    info.width > MAX_IMAGE_SIDE_PX || info.height > MAX_IMAGE_SIDE_PX || info.width * info.height > MAX_IMAGE_PIXELS;
  return isTooBig ? null : info.type;
}

/**
 * A GIF's screen size, grown to its first frame's size: decoders make room
 * for the frame even when it's larger than the screen it claims.
 */
function gifInfo(bytes: Uint8Array, view: DataView): ImageInfo | null {
  let width = view.getUint16(6, true);
  let height = view.getUint16(8, true);
  const flags = bytes[10];
  let offset = 13 + (flags & 0x80 ? 3 * 2 ** ((flags & 0x07) + 1) : 0);
  while (offset < bytes.length) {
    const block = bytes[offset];
    if (block === 0x2c) {
      if (offset + 9 > bytes.length) return null;
      width = Math.max(width, view.getUint16(offset + 5, true));
      height = Math.max(height, view.getUint16(offset + 7, true));
      return { type: "image/gif", width, height };
    }
    if (block !== 0x21) return null;
    // An extension: its label, then sub-blocks until an empty one.
    offset += 2;
    while (offset < bytes.length && bytes[offset] !== 0) offset += bytes[offset] + 1;
    offset += 1;
  }
  return null;
}

function webpInfo(bytes: Uint8Array, view: DataView): ImageInfo | null {
  const chunk = String.fromCharCode(...bytes.subarray(12, 16));
  if (chunk === "VP8X") {
    const width = 1 + (bytes[24] | (bytes[25] << 8) | (bytes[26] << 16));
    const height = 1 + (bytes[27] | (bytes[28] << 8) | (bytes[29] << 16));
    return { type: "image/webp", width, height };
  }
  if (chunk === "VP8L") {
    const bits = view.getUint32(21, true);
    return { type: "image/webp", width: (bits & 0x3fff) + 1, height: ((bits >> 14) & 0x3fff) + 1 };
  }
  if (chunk === "VP8 ") {
    return { type: "image/webp", width: view.getUint16(26, true) & 0x3fff, height: view.getUint16(28, true) & 0x3fff };
  }
  return null;
}

const isStandalone = (marker: number) => marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7);
const isStartOfFrame = (marker: number) =>
  marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc;

/** A JPEG's size, from its first start-of-frame marker, walking markers the way decoders do. */
function jpegInfo(bytes: Uint8Array, view: DataView): ImageInfo | null {
  let offset = 2;
  while (offset < bytes.length) {
    if (bytes[offset] !== 0xff) return null;
    // Any number of 0xFF fill bytes may come before a marker.
    while (bytes[offset] === 0xff) offset += 1;
    if (offset >= bytes.length) return null;
    const marker = bytes[offset];
    offset += 1;
    if (isStandalone(marker)) continue;
    if (marker === 0x00 || marker === 0xd8 || marker === 0xd9 || offset + 2 > bytes.length) return null;
    if (isStartOfFrame(marker)) {
      if (offset + 7 > bytes.length) return null;
      return { type: "image/jpeg", width: view.getUint16(offset + 5), height: view.getUint16(offset + 3) };
    }
    const length = view.getUint16(offset);
    if (length < 2) return null;
    offset += length;
  }
  return null;
}
