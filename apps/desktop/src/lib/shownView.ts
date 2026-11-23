/**
 * Which workspace and view the window shows right now, for what lives above
 * the shell (notifications): whether you're already looking at a channel, and
 * how to open something there. Null while no workspace is shown, as while
 * adding one.
 */
import type { View } from "./workspace";

export type ShownView = {
  workspaceId: string;
  view: () => View | null;
  navigate: (view: View) => void;
};

let shown: ShownView | null = null;

/** Called by the shell while it's mounted; the returned function unregisters it. */
export function showView(view: ShownView): () => void {
  shown = view;
  return () => {
    if (shown === view) shown = null;
  };
}

export function shownView(): ShownView | null {
  return shown;
}
