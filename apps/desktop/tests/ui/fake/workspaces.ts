/**
 * Several workspaces on one device, in the fake: the rail's list, badges,
 * mute, the global pause, and leaving. Every workspace shares the fake's one
 * set of channels and messages; selecting one swaps the session, which is
 * what the screens see.
 */

import type { InvokeArgs } from "@tauri-apps/api/core";
import type { Change, Session, WorkspaceBadge, WorkspaceItem } from "../../../src/lib/api";
import { argument, requireString } from "./helpers";

/** Returned by `answer` for commands that aren't about workspaces. */
export const NOT_WORKSPACES = Symbol("not a workspaces command");

export type WorkspacesContext = {
  session: () => Session | null;
  setSession: (session: Session | null) => void;
  /** Sends `change` as coming from `workspaceId` (null for the app itself). */
  emitFrom: (workspaceId: string | null, change: Change) => void;
  /** The shown workspace's own badge, counted from the fake's messages as the core counts it. */
  shownBadge: () => Omit<WorkspaceBadge, "workspace_id">;
};

export class FakeWorkspaces {
  private readonly context: WorkspacesContext;
  /** Workspaces besides the one the fake started in, which joins the list on first use. */
  private list: WorkspaceItem[] | null = null;
  private badges = new Map<string, Omit<WorkspaceBadge, "workspace_id">>();
  /** Workspaces marked read with "Mark all as read", in order. */
  readonly markedRead: string[] = [];
  /** Makes the next `leave_workspace` fail with this, as a folder still in use would. */
  leaveFails: string | null = null;
  /** Until when every workspace's notifications are held, as `set_notifications_paused` left it. */
  pausedUntil: number | null = null;

  constructor(context: WorkspacesContext) {
    this.context = context;
  }

  // ---- Acting as the device, for tests ----------------------------------

  /** Another workspace on this device, joined earlier; returns its id. */
  add(name: string): string {
    const id = `w-${name.toLowerCase().replaceAll(/\W+/g, "-")}`;
    this.items().push({ workspace_id: id, name, icon: null, is_live: true, is_muted: false });
    this.context.emitFrom(null, { kind: "workspaces" });
    return id;
  }

  /** What `workspaceId`'s rail icon shows, as its own unread messages would make it. */
  setBadge(workspaceId: string, unread: boolean, mentions: number, attention = mentions): void {
    this.badges.set(workspaceId, { unread, mentions, attention });
    this.context.emitFrom(workspaceId, { kind: "channels" });
  }

  /** Whether the core would hold back a notification from `workspaceId`. */
  isSilenced(workspaceId: string): boolean {
    const isPaused = this.pausedUntil !== null && this.pausedUntil > Date.now();
    return isPaused || Boolean(this.items().find((item) => item.workspace_id === workspaceId)?.is_muted);
  }

  // ---- The command surface ----------------------------------------------

  answer(command: string, args: InvokeArgs | undefined): unknown {
    switch (command) {
      case "workspaces":
        return this.items();
      case "workspace_badges":
        return this.items().map((item) => ({
          workspace_id: item.workspace_id,
          ...(item.workspace_id === this.context.session()?.workspace_id
            ? this.context.shownBadge()
            : (this.badges.get(item.workspace_id) ?? { unread: false, mentions: 0, attention: 0 })),
        }));
      case "select_workspace":
        return this.select(requireString(args, "workspaceId"));
      case "move_workspace":
        return this.move(requireString(args, "workspaceId"), Number(argument(args, "position")));
      case "leave_workspace":
        return this.leave(requireString(args, "workspaceId"));
      case "set_workspace_muted":
        this.item(requireString(args, "workspaceId")).is_muted = argument(args, "muted") === true;
        this.context.emitFrom(null, { kind: "workspaces" });
        return null;
      case "mark_workspace_read": {
        const id = requireString(args, "workspaceId");
        this.markedRead.push(id);
        this.badges.set(id, { unread: false, mentions: 0, attention: 0 });
        this.context.emitFrom(id, { kind: "channels" });
        return null;
      }
      case "set_notifications_paused": {
        const until = argument(args, "untilMs");
        this.pausedUntil = typeof until === "number" ? until : null;
        return null;
      }
      case "notifications_paused":
        return this.pausedUntil;
      default:
        return NOT_WORKSPACES;
    }
  }

  private items(): WorkspaceItem[] {
    if (this.list) return this.list;
    const session = this.context.session();
    this.list = session
      ? [
          {
            workspace_id: session.workspace_id,
            name: session.workspace_name,
            icon: null,
            is_live: true,
            is_muted: false,
          },
        ]
      : [];
    return this.list;
  }

  private item(id: string): WorkspaceItem {
    const found = this.items().find((item) => item.workspace_id === id);
    if (!found) throw new Error(`fake backend: no workspace ${id}`);
    return found;
  }

  private select(id: string): Session {
    const chosen = this.item(id);
    const current = this.context.session();
    if (!current) throw new Error("fake backend: no workspace");
    const session = { ...current, workspace_id: chosen.workspace_id, workspace_name: chosen.name };
    this.context.setSession(session);
    this.context.emitFrom(null, { kind: "workspaces" });
    return session;
  }

  private move(id: string, position: number): null {
    const items = this.items();
    const moved = this.item(id);
    items.splice(items.indexOf(moved), 1);
    items.splice(position, 0, moved);
    this.context.emitFrom(null, { kind: "workspaces" });
    return null;
  }

  /** As in the core: gone for good, and the next one is shown if the shown one went. */
  private leave(id: string): Session | null {
    if (this.leaveFails !== null) {
      const message = this.leaveFails;
      this.leaveFails = null;
      this.context.emitFrom(null, { kind: "workspaces" });
      throw new Error(message);
    }
    const items = this.items();
    items.splice(items.indexOf(this.item(id)), 1);
    this.badges.delete(id);
    const current = this.context.session();
    const next = current?.workspace_id === id ? (items[0] ? this.select(items[0].workspace_id) : null) : current;
    if (next === null) this.context.setSession(null);
    this.context.emitFrom(null, { kind: "workspaces" });
    return next;
  }
}
