import { createContext, useContext } from "react";
import type { Channel, LaterItem, LinkedMessage, Page, Session } from "./api";

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

/** The channel with a linked message shown in place, in its thread if it's a reply. */
export function linkedMessageView({ message, thread_root }: LinkedMessage): View {
  return { kind: "channel", id: message.channel, thread: thread_root ?? undefined, focus: message.id };
}

/** Where a saved item lives: its message, or its page. */
export function savedItemView(item: LaterItem): View | null {
  if (item.message) return linkedMessageView(item.message);
  if (item.page) return { kind: "page", id: item.page.id };
  return null;
}

export function sameView(left: View | null, right: View | null) {
  return left !== null && right !== null && viewKey(left) === viewKey(right);
}

/** Channels (in the order their sections show them) then pages, as in the sidebar. */
export function sidebarViews(channelOrder: string[], pages: Page[]): View[] {
  const ordered: View[] = channelOrder.map((id) => ({ kind: "channel", id }));
  const addPages = (parent: string | null) => {
    for (const page of pages.filter((candidate) => candidate.parent === parent)) {
      ordered.push({ kind: "page", id: page.id });
      addPages(page.id);
    }
  };
  addPages(null);
  return ordered;
}
