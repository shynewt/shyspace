/** Putting files into a page: pasted or dropped, uploaded, then linked in its text. */
import type { EditorView } from "@codemirror/view";
import { api, onChange, type StagedFile } from "../../lib/api";
import { MAX_PASTED_BYTES } from "../../lib/limits";
import { reportError } from "../../lib/toast";
import { finishUpload, showUploadProgress, startUpload } from "./editor/uploads";

/** What browsers call an image pasted from the clipboard, which says nothing about it. */
const CLIPBOARD_IMAGE_NAME = /^image\.(png|jpe?g|gif|webp)$/i;
/** Shown inline in the page; anything else is linked by its name. */
const INLINE_IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);

/** Uploads `files` one after another, each linked where the cursor was when it started. */
export async function uploadIntoPage(view: EditorView, pageId: string, files: StagedFile[]): Promise<void> {
  for (const file of files) {
    const id = startUpload(view, file.name, view.state.selection.main.head);
    const stopProgress = onChange((change) => {
      if (change.kind === "upload" && change.channel === pageId && change.total > 0) {
        showUploadProgress(view, id, change.sent / change.total);
      }
    });
    try {
      const fileId = await api.sharePageFile(pageId, file.token);
      finishUpload(view, id, pageFileLink(file, fileId));
    } catch (error) {
      finishUpload(view, id, null);
      api.unstageFile(file.token).catch(reportError);
      reportError(error);
    } finally {
      stopProgress();
    }
  }
}

/** Stages pasted files for the page, naming a bare clipboard image after the page. */
export async function stagePastedForPage(files: File[], pageId: string, pageTitle: string): Promise<StagedFile[]> {
  const staged: StagedFile[] = [];
  let imagesNamed = 0;
  for (const file of files) {
    if (file.size > MAX_PASTED_BYTES) {
      reportError(`${file.name} is too large to paste. Drop it on the page instead.`);
      continue;
    }
    let named = file;
    if (CLIPBOARD_IMAGE_NAME.test(file.name)) {
      imagesNamed += 1;
      named = new File([file], await pastedImageName(file.name, pageId, pageTitle, imagesNamed), { type: file.type });
    }
    staged.push(await api.stagePastedFile(named));
  }
  return staged;
}

/** "Plan image 3.png": the page's title and the next free number among its images named so. */
async function pastedImageName(original: string, pageId: string, pageTitle: string, offset: number): Promise<string> {
  const extension = original.slice(original.lastIndexOf("."));
  const base = `${pageTitle.trim() || "Untitled"} image `;
  const taken = (await api.pageFiles(pageId)).filter((file) => file.name.startsWith(base)).length;
  return `${base}${taken + offset}${extension}`;
}

/** The markdown linking a page's file: an image shows inline, anything else by its name. */
export function pageFileLink(file: { name: string; media_type: string }, fileId: string): string {
  const url = `shy://file/${fileId}`;
  return INLINE_IMAGE_TYPES.has(file.media_type) ? `![](${url})` : `[${escapeLinkText(file.name)}](${url})`;
}

/** Text that stays plain inside a link's brackets: no early `]`, code span or line break. */
export function escapeLinkText(text: string): string {
  return text.replace(/[\\[\]`]/g, (char) => `\\${char}`).replace(/\s*\n\s*/g, " ");
}
