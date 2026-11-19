import { createContext, useContext } from "react";
import type { Channel, Page, Session } from "./api";

export type View =
  /** `thread` opens that thread beside the channel; `focus` scrolls to and highlights a message. */
  | { kind: "channel"; id: string; thread?: string; focus?: string }
  | { kind: "page"; id: string }
  | { kind: "inbox" }
  | { kind: "later" };

export function viewKey(view: View): string {
  return view.kind === "inbox" || view.kind === "later" ? view.kind : `${view.kind}:${view.id}`;
}

export type WorkspaceContextValue = {
  session: Session;
  channels: Channel[];
  pages: Page[];
  view: View | null;
  navigate: (view: View) => void;
};

export const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);

export function useWorkspace() {
  const value = useContext(WorkspaceContext);
  if (!value) throw new Error("useWorkspace must be used inside the workspace shell");
  return value;
}

export function sameView(left: View | null, right: View | null) {
  return left !== null && right !== null && viewKey(left) === viewKey(right);
}

/** Channels then pages (as the page tree lists them), in sidebar order. */
export function sidebarViews(channels: Channel[], pages: Page[]): View[] {
  const ordered: View[] = channels.map((channel) => ({ kind: "channel", id: channel.id }));
  const addPages = (parent: string | null) => {
    for (const page of pages.filter((candidate) => candidate.parent === parent)) {
      ordered.push({ kind: "page", id: page.id });
      addPages(page.id);
    }
  };
  addPages(null);
  return ordered;
}
