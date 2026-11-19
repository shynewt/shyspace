import { createContext, useContext } from "react";
import type { Channel, Page, Session } from "./api";

export type View =
  /** `thread` opens that thread beside the channel. */
  { kind: "channel"; id: string; thread?: string } | { kind: "page"; id: string } | { kind: "inbox" };

export function viewKey(view: View): string {
  return view.kind === "inbox" ? "inbox" : `${view.kind}:${view.id}`;
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
