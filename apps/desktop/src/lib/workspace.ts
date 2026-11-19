import { createContext, useContext } from "react";
import type { Channel, Page, Session } from "./api";

export type View = { kind: "channel"; id: string } | { kind: "page"; id: string };

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
  return left?.kind === right?.kind && left?.id === right?.id;
}
