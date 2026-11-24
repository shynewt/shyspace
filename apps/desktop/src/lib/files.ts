import { useEffect, useRef, useState } from "react";
import { api, type FileInfo, type StagedFile, shownWorkspaceId } from "./api";
import { showableImageType } from "./imageHeaders";

/**
 * Images up to this size load and show by themselves; bigger ones are a card
 * you save on purpose, so nobody can make every reader download gigabytes.
 */
export const MAX_INLINE_IMAGE_BYTES = 5 * 1024 * 1024;
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
 * Loaded pictures for the whole app, by workspace, member and picture id: ids
 * are public, so someone could reuse another member's under their own name.
 * A handful per member at most, so they're kept for the session. Only a
 * picture that loaded is kept: one whose bytes haven't arrived yet, or that
 * loaded while the workspace was switched, is asked for again.
 */
const pictures = new Map<string, Promise<string | null>>();

/** A member's picture as an image URL, once loaded. */
export function usePicture(member: string, picture: string | null): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!picture) return setUrl(null);
    let isCurrent = true;
    const workspaceId = shownWorkspaceId();
    const key = `${workspaceId}:${member}:${picture}`;
    let loading = pictures.get(key);
    if (!loading) {
      loading = api.profilePicture(member).then((bytes) => {
        const type = showableImageType(bytes);
        return type ? URL.createObjectURL(new Blob([bytes as BlobPart], { type })) : null;
      });
      pictures.set(key, loading);
      // A failure (say, offline), nothing to show, or an answer from the workspace
      // switched to meanwhile is tried again next time it's shown.
      loading.then(
        (loaded) => (loaded === null || shownWorkspaceId() !== workspaceId) && pictures.delete(key),
        () => pictures.delete(key),
      );
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
