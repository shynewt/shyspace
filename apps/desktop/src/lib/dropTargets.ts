/**
 * Places that take files dropped on the window: message boxes, and an open
 * page. A drop goes to the one in the panel under it (the channel, its
 * thread, a DM, a page); a drop elsewhere, like on the sidebar, goes to the
 * one used last, and when that one closes, to the one used before it.
 */
import { api, type DropPoint, onFilesDropped } from "./api";
import { reportError } from "./toast";

/** Marks a panel whose own message box or page takes what's dropped on it. */
export const DROP_ZONE = "data-drop-zone";

type Target = { id: string; element: () => Element | null };

const targets: Target[] = [];
let isFallbackListening = false;

/** The target for a drop at `position`: the one in the panel there, else the one used last. */
export function dropTargetAt(position: DropPoint | null): string | null {
  const zone = position ? document.elementFromPoint(position.x, position.y)?.closest(`[${DROP_ZONE}]`) : null;
  const inZone = zone ? targets.findLast((target) => zone.contains(target.element())) : undefined;
  return (inZone ?? targets.at(-1))?.id ?? null;
}

/** A target that appears takes drops only once used, unless nothing else is there. */
export function addDropTarget(id: string, element: () => Element | null): void {
  listenForUnclaimedDrops();
  if (!targets.some((target) => target.id === id)) targets.unshift({ id, element });
}

export function removeDropTarget(id: string): void {
  const index = targets.findIndex((target) => target.id === id);
  if (index !== -1) targets.splice(index, 1);
}

export function markDropTargetUsed(id: string): void {
  const index = targets.findIndex((target) => target.id === id);
  if (index === -1) return;
  const [target] = targets.splice(index, 1);
  targets.push(target);
}

/** Drops with nowhere to go (the Files view, say) are let go, and say why. */
function listenForUnclaimedDrops() {
  if (isFallbackListening) return;
  isFallbackListening = true;
  onFilesDropped(({ files, position }) => {
    if (dropTargetAt(position) !== null) return;
    for (const file of files) api.unstageFile(file.token).catch(reportError);
    reportError("Open a channel, direct message or page to attach files.");
  });
}
