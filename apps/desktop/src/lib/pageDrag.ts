/**
 * Dragging a page out of the sidebar. The page travels as its id, for the
 * message box to mention it, and as its link, so other text fields (a page
 * being edited) take it too.
 */
import type { DragEvent } from "react";
import { PAGE_LINK_PREFIX } from "./api";

const PAGE_DRAG_TYPE = "application/x-shy-page";

export function startPageDrag(event: DragEvent, pageId: string) {
  event.dataTransfer.setData(PAGE_DRAG_TYPE, pageId);
  event.dataTransfer.setData("text/plain", `${PAGE_LINK_PREFIX}${pageId}`);
  event.dataTransfer.effectAllowed = "copyLink";
}

/** While dragging, only the kind of data can be read, not the page itself. */
export function isPageDrag(event: DragEvent): boolean {
  return event.dataTransfer.types.includes(PAGE_DRAG_TYPE);
}

export function droppedPageId(event: DragEvent): string | null {
  return event.dataTransfer.getData(PAGE_DRAG_TYPE) || null;
}
