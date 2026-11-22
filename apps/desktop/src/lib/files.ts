import { useEffect, useRef, useState } from "react";
import { api, type FileInfo, type StagedFile } from "./api";

/**
 * Images up to this size load and show by themselves; bigger ones are a card
 * you save on purpose, so nobody can make every reader download gigabytes.
 */
export const MAX_INLINE_IMAGE_BYTES = 5 * 1024 * 1024;
/** Decoding is what costs memory, so an image's own size claim is checked first. */
const MAX_IMAGE_SIDE_PX = 8192;
const MAX_IMAGE_PIXELS = 40_000_000;

type ImageInfo = { type: string; width: number; height: number };

/**
 * What these bytes really are, if a PNG, JPEG, GIF or WebP image, with the
 * size its header claims. Never SVG or anything else that can run code.
 */
export function imageInfoOf(bytes: Uint8Array): ImageInfo | null {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const has = (offset: number, prefix: number[]) => prefix.every((byte, index) => bytes[offset + index] === byte);
  if (bytes.length >= 24 && has(0, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) {
    return { type: "image/png", width: view.getUint32(16), height: view.getUint32(20) };
  }
  if (bytes.length >= 10 && has(0, [0x47, 0x49, 0x46, 0x38])) {
    return { type: "image/gif", width: view.getUint16(6, true), height: view.getUint16(8, true) };
  }
  if (bytes.length >= 30 && has(0, [0x52, 0x49, 0x46, 0x46]) && has(8, [0x57, 0x45, 0x42, 0x50])) {
    return webpInfo(bytes, view);
  }
  if (bytes.length >= 4 && has(0, [0xff, 0xd8, 0xff])) return jpegInfo(bytes, view);
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

/** A JPEG's size sits in its first start-of-frame marker. */
function jpegInfo(bytes: Uint8Array, view: DataView): ImageInfo | null {
  let offset = 2;
  while (offset + 9 < bytes.length) {
    if (bytes[offset] !== 0xff) return null;
    const marker = bytes[offset + 1];
    const isStartOfFrame = marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc;
    if (isStartOfFrame) {
      return { type: "image/jpeg", width: view.getUint16(offset + 7), height: view.getUint16(offset + 5) };
    }
    offset += 2 + view.getUint16(offset + 2);
  }
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

/** Whether a file may be an image worth fetching to show; its bytes decide in the end. */
export function mayShowInline(file: FileInfo): boolean {
  return file.media_type.startsWith("image/") && file.size <= MAX_INLINE_IMAGE_BYTES;
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  const megabytes = bytes / 1024 / 1024;
  return `${megabytes < 10 ? megabytes.toFixed(1) : Math.round(megabytes)} MB`;
}

export type LoadedImage =
  | { status: "none" | "loading" }
  | { status: "shown"; url: string }
  | { status: "not-an-image" | "failed" };

/** Where an image's bytes come from; `key` changes when they do. */
export type ImageSource = { key: string; load: () => Promise<Uint8Array> };

/** Bytes as an image URL, if they are an image this app shows. Nothing is loaded without a source. */
export function useImage(source: ImageSource | null): LoadedImage {
  const [image, setImage] = useState<LoadedImage>({ status: "none" });
  const key = source?.key ?? null;
  const load = useRef(source?.load);
  load.current = source?.load;
  useEffect(() => {
    const loadBytes = load.current;
    if (key === null || !loadBytes) return setImage({ status: "none" });
    setImage({ status: "loading" });
    let url: string | null = null;
    let isCurrent = true;
    loadBytes().then(
      (bytes) => {
        if (!isCurrent) return;
        const type = showableImageType(bytes);
        if (!type) return setImage({ status: "not-an-image" });
        url = URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
        setImage({ status: "shown", url });
      },
      () => isCurrent && setImage({ status: "failed" }),
    );
    return () => {
      isCurrent = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [key]);
  return image;
}

/** A shared file as an image, if it may be one small enough to load by itself. */
export function useFileImage(file: FileInfo): LoadedImage {
  return useImage(mayShowInline(file) ? { key: file.id, load: () => api.fileBytes(file.id) } : null);
}

/** A file waiting in the message box as an image, read through the native side. */
export function useStagedImage(file: StagedFile): LoadedImage {
  const isCandidate = file.media_type.startsWith("image/") && file.size <= MAX_INLINE_IMAGE_BYTES;
  return useImage(isCandidate ? { key: file.token, load: () => api.stagedFileBytes(file.token) } : null);
}

/**
 * Loaded pictures for the whole app, by member and picture id: ids are
 * public, so someone could reuse another member's under their own name.
 * A handful per member at most, so they're kept for the session.
 */
const pictures = new Map<string, Promise<string | null>>();

/** A member's picture as an image URL, once loaded. */
export function usePicture(member: string, picture: string | null): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!picture) return setUrl(null);
    let isCurrent = true;
    const key = `${member}:${picture}`;
    let loading = pictures.get(key);
    if (!loading) {
      loading = api.profilePicture(member).then((bytes) => {
        const type = showableImageType(bytes);
        return type ? URL.createObjectURL(new Blob([bytes as BlobPart], { type })) : null;
      });
      pictures.set(key, loading);
      // A failure (say, offline) is tried again next time it's shown.
      loading.catch(() => pictures.delete(key));
    }
    loading.then(
      (loaded) => isCurrent && setUrl(loaded),
      () => isCurrent && setUrl(null),
    );
    return () => {
      isCurrent = false;
    };
  }, [member, picture]);
  return url;
}
