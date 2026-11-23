import { useCallback, useEffect, useId, useRef, useState } from "react";
import { api, onChange, onFileDrag, onFilesDropped, type StagedFile } from "../../lib/api";
import { activeDropTarget, addDropTarget, markDropTargetUsed, removeDropTarget } from "../../lib/dropTargets";
import { formatSize } from "../../lib/files";
import { MAX_FILE_BYTES, MAX_FILES_PER_MESSAGE, MAX_PASTED_BYTES } from "../../lib/limits";
import { reportError } from "../../lib/toast";

export type Upload = { sent: number; total: number };

/** Files waiting in a message box: picked, dropped or pasted, until sent or removed. */
export function useAttachments(channelId: string) {
  const boxId = useId();
  const [staged, setStaged] = useState<StagedFile[]>([]);
  const [upload, setUpload] = useState<Upload | null>(null);
  const [isDragTarget, setDragTarget] = useState(false);
  const isSending = useRef(false);
  const stagedNow = useRef(staged);
  stagedNow.current = staged;

  const isMounted = useRef(true);

  useEffect(() => {
    isMounted.current = true;
    // A box focused while mounting has already put itself last.
    addDropTarget(boxId);
    return () => {
      isMounted.current = false;
      removeDropTarget(boxId);
      // Leaving the box lets its files go; a send already under way has its own copy.
      for (const file of stagedNow.current) api.unstageFile(file.token).catch(reportError);
    };
  }, [boxId]);
  const markActive = useCallback(() => markDropTargetUsed(boxId), [boxId]);

  const add = useCallback((files: StagedFile[]) => setStaged((current) => [...current, ...files]), []);
  useEffect(() => onFilesDropped((files) => activeDropTarget() === boxId && add(files)), [boxId, add]);
  useEffect(() => onFileDrag((isDragging) => setDragTarget(isDragging && activeDropTarget() === boxId)), [boxId]);
  useEffect(
    () =>
      onChange((change) => {
        // Another box for this channel may be sending; only this box's own upload counts.
        if (change.kind === "upload" && change.channel === channelId && isSending.current) {
          setUpload({ sent: change.sent, total: change.total });
        }
      }),
    [channelId],
  );

  const remove = useCallback((token: string) => {
    setStaged((current) => current.filter((file) => file.token !== token));
    api.unstageFile(token).catch(reportError);
  }, []);

  const pick = useCallback(async () => {
    try {
      add(await api.pickFiles());
    } catch (error) {
      reportError(error);
    }
  }, [add]);

  /** Stages files pasted into the box; true if there were any. */
  const paste = useCallback(
    (files: File[]) => {
      if (files.length === 0) return false;
      for (const file of files) {
        if (file.size > MAX_PASTED_BYTES) {
          reportError(`${file.name} is too large to paste. Attach it instead.`);
          continue;
        }
        api.stagePastedFile(file).then((stagedFile) => {
          // The box may have closed while the bytes crossed over.
          if (isMounted.current) add([stagedFile]);
          else api.unstageFile(stagedFile.token).catch(reportError);
        }, reportError);
      }
      return true;
    },
    [add],
  );

  const problem =
    staged.length > MAX_FILES_PER_MESSAGE
      ? `A message carries at most ${MAX_FILES_PER_MESSAGE} files.`
      : staged.some((file) => file.size > MAX_FILE_BYTES)
        ? `Files can be at most ${formatSize(MAX_FILE_BYTES)}.`
        : null;

  /** Uploads the files and sends them with `text`; they leave the box once sent. */
  const send = useCallback(
    async (text: string, replyTo: string | null) => {
      const sending = staged.map((file) => file.token);
      isSending.current = true;
      setUpload({ sent: 0, total: staged.reduce((sum, file) => sum + file.size, 0) });
      try {
        await api.sendFiles(channelId, text, replyTo, sending);
        // Files added while it uploaded stay for the next message.
        setStaged((current) => current.filter((file) => !sending.includes(file.token)));
      } finally {
        isSending.current = false;
        setUpload(null);
      }
    },
    [channelId, staged],
  );

  return { staged, upload, isDragTarget, problem, markActive, remove, pick, paste, send };
}
