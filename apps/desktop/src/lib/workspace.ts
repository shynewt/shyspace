import { createContext, useContext } from "react";
import type { Channel, LaterItem, LinkedMessage, Member, Page, Session } from "./api";
import type { SettingsTarget } from "./settings";

export type View =
  /** `thread` opens that thread beside the channel; `focus` scrolls to and highlights a message. */
  | { kind: "channel"; id: string; thread?: string; focus?: string }
  /** `at` puts the caret at that spot (an encoded Yjs relative position) and shows it. */
  | { kind: "page"; id: string; at?: number[] }
  | { kind: "inbox" }
  | { kind: "later" }
  | { kind: "files" }
  /** Drafts & sent. */
  | { kind: "drafts" };

export function viewKey(view: View): string {
  return view.kind === "channel" || view.kind === "page" ? `${view.kind}:${view.id}` : view.kind;
}

export type WorkspaceContextValue = {
  session: Session;
  channels: Channel[];
  pages: Page[];
  /** Everyone, former members too, with ended statuses dropped; undefined until loaded. */
  members: Member[] | undefined;
  view: View | null;
  navigate: (view: View) => void;
  /** Opens Settings, at Profile unless a section is given. */
  openSettings: (target?: SettingsTarget) => void;
  /** Opens the search palette, with `query` typed in. */
  openSearch: (query?: string) => void;
  /** Opens the dialog for setting your status. */
  openStatus: () => void;
  /** Shows everything a member shares; focus goes to `returnFocusTo` when it closes. */
  openProfile: (memberId: string, returnFocusTo?: HTMLElement | null) => void;
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
