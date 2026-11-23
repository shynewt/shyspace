/**
 * Places that take files dropped on the window: message boxes, and an open
 * page. The one used last is at the end and gets the drop; when it closes,
 * the one used before it does.
 */
import { api, onFilesDropped } from "./api";
import { reportError } from "./toast";

const targets: string[] = [];
let isFallbackListening = false;

export function activeDropTarget(): string | null {
  return targets.at(-1) ?? null;
}

/** A target that appears takes drops only once used, unless nothing else is there. */
export function addDropTarget(target: string): void {
  listenForUnclaimedDrops();
  if (!targets.includes(target)) targets.unshift(target);
}

export function removeDropTarget(target: string): void {
  const index = targets.indexOf(target);
  if (index !== -1) targets.splice(index, 1);
}

export function markDropTargetUsed(target: string): void {
  removeDropTarget(target);
  targets.push(target);
}

/** Drops with nowhere to go (the Files view, say) are let go, and say why. */
function listenForUnclaimedDrops() {
  if (isFallbackListening) return;
  isFallbackListening = true;
  onFilesDropped((files) => {
    if (activeDropTarget() !== null) return;
    for (const file of files) api.unstageFile(file.token).catch(reportError);
    reportError("Open a channel, direct message or page to attach files.");
  });
}
