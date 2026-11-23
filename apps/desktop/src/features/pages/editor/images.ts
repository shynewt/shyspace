/**
 * Images of a page's own files: `![caption](shy://file/<id>#width=N)`. The
 * caption is the alt text; the width, a whole percent of the text column from
 * 10 to 100, rides in the fragment, which every other markdown viewer ignores.
 */
import { syntaxTree } from "@codemirror/language";
import { type EditorState, Facet } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { escapeLinkText } from "../pageUploads";

const FILE_IMAGE_URL = /^shy:\/\/file\/([0-9a-f]+)(#.*)?$/;
const WIDTH_FRAGMENT = /^#width=(\d{2,3})$/;
export const MIN_IMAGE_WIDTH = 10;
export const MAX_IMAGE_WIDTH = 100;

/** The file an image URL names, and its width when the fragment is one we write. */
export function parseFileImageUrl(url: string): { fileId: string; width: number | null } | null {
  const match = url.match(FILE_IMAGE_URL);
  if (!match) return null;
  const width = Number(match[2]?.match(WIDTH_FRAGMENT)?.[1]);
  const isWidth = Number.isInteger(width) && width >= MIN_IMAGE_WIDTH && width <= MAX_IMAGE_WIDTH;
  return { fileId: match[1], width: isWidth ? width : null };
}

/** Alt text as written, back to what it says: `\[` reads `[`. */
export function unescapeLinkText(text: string): string {
  return text.replace(/\\([\\[\]`])/g, "$1");
}

/** Where an image's parts sit in the document, for rewriting one of them. */
export type ImageRange = {
  from: number;
  to: number;
  captionFrom: number;
  captionTo: number;
  urlFrom: number;
  urlTo: number;
  fileId: string;
  width: number | null;
};

/** The page-file image at or right around `position`, if there is one. */
export function imageAt(state: EditorState, position: number): ImageRange | null {
  const tree = syntaxTree(state);
  for (const side of [1, -1] as const) {
    for (
      let node: ReturnType<typeof tree.resolveInner> | null = tree.resolveInner(position, side);
      node;
      node = node.parent
    ) {
      if (node.name !== "Image") continue;
      const url = node.getChild("URL");
      const marks = node.getChildren("LinkMark");
      if (!url || marks.length < 2) return null;
      const parsed = parseFileImageUrl(state.sliceDoc(url.from, url.to));
      if (!parsed) return null;
      return {
        from: node.from,
        to: node.to,
        captionFrom: marks[0].to,
        captionTo: marks[1].from,
        urlFrom: url.from,
        urlTo: url.to,
        ...parsed,
      };
    }
  }
  return null;
}

/** The image a selection covers exactly, as it does after Shift+arrow over one. */
export function selectedImage(state: EditorState): ImageRange | null {
  const selection = state.selection.main;
  if (selection.empty) return null;
  const image = imageAt(state, selection.from);
  return image && image.from === selection.from && image.to === selection.to ? image : null;
}

/** Writes a new width, or none (the image's own size, at most the column's). */
export function setImageWidth(view: EditorView, image: ImageRange, width: number | null): void {
  const clamped = width === null ? null : Math.min(MAX_IMAGE_WIDTH, Math.max(MIN_IMAGE_WIDTH, Math.round(width)));
  const url = `shy://file/${image.fileId}${clamped === null ? "" : `#width=${clamped}`}`;
  view.dispatch({
    changes: { from: image.urlFrom, to: image.urlTo, insert: url },
    // The image stays selected, so the next Alt+arrow resizes it again.
    selection: { anchor: image.from, head: image.to + url.length - (image.urlTo - image.urlFrom) },
    userEvent: "input.format",
  });
}

export function setImageCaption(view: EditorView, image: ImageRange, caption: string): void {
  view.dispatch({
    changes: { from: image.captionFrom, to: image.captionTo, insert: escapeLinkText(caption.trim()) },
    userEvent: "input.format",
  });
}

/** What the page shows for an image; the screen that hosts the editor draws it. */
export type ImageProps = { fileId: string; width: number | null; caption: string };

/** Where image widgets ask to be drawn, and say they're gone. */
export type ImageHost = {
  show: (element: HTMLElement, props: ImageProps) => void;
  hide: (element: HTMLElement) => void;
};

export const imageHost = Facet.define<ImageHost, ImageHost | undefined>({
  combine: (hosts) => hosts[0],
});
