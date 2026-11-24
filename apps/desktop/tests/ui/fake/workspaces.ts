/**
 * Several workspaces on one device, in the fake: the rail's list, badges,
 * mute, the global pause, and leaving. Every workspace shares the fake's one
 * set of channels and messages; selecting one swaps the session, which is
 * what the screens see.
 */

import type { InvokeArgs } from "@tauri-apps/api/core";
import type { Change, Session, WorkspaceBadge, WorkspaceItem, WorkspaceLook } from "../../../src/lib/api";
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
  private looks = new Map<string, WorkspaceLook>();
  private icons = new Map<string, number[]>();

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

  /** The admin of `workspaceId` chose this look, as their op arriving would set it. */
  receiveLook(workspaceId: string, look: WorkspaceLook): void {
    this.looks.set(workspaceId, look);
    this.context.emitFrom(workspaceId, { kind: "workspace_look" });
  }

  /** The admin of `workspaceId` set its icon (empty for the letter), as their op arriving would. */
  receiveIcon(workspaceId: string, image: number[]): void {
    this.setIcon(workspaceId, image);
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
      case "workspace_look":
        return this.looks.get(this.shownId()) ?? { theme: null, accent: null };
      case "set_workspace_look": {
        this.requireAdmin();
        const look = { theme: lookName(argument(args, "theme")), accent: lookName(argument(args, "accent")) };
        this.looks.set(this.shownId(), look);
        this.context.emitFrom(this.shownId(), { kind: "workspace_look" });
        return null;
      }
      case "workspace_icon":
        return new Uint8Array(this.icons.get(requireString(args, "workspaceId")) ?? []).buffer;
      case "set_workspace_icon": {
        this.requireAdmin();
        const image = argument(args, "image");
        if (!Array.isArray(image)) throw new Error("fake backend: set_workspace_icon takes bytes");
        this.setIcon(this.shownId(), image);
        return null;
      }
      default:
        return NOT_WORKSPACES;
    }
  }

  private shownId(): string {
    const id = this.context.session()?.workspace_id;
    if (!id) throw new Error("fake backend: no workspace");
    return id;
  }

  private requireAdmin(): void {
    if (!this.context.session()?.is_admin)
      throw new Error("only the workspace admin can change the workspace's icon and look");
  }

  private setIcon(workspaceId: string, image: number[]): void {
    if (image.length === 0) this.icons.delete(workspaceId);
    else this.icons.set(workspaceId, image);
    this.item(workspaceId).icon = image.length === 0 ? null : `icon-${Math.random().toString(16).slice(2)}`;
    this.context.emitFrom(workspaceId, { kind: "workspaces" });
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

/** As in the core: a theme or accent name is a-z, 1 to 24 letters, or null for none. */
function lookName(value: unknown): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "string" || !/^[a-z]{1,24}$/.test(value))
    throw new Error(`not a theme or accent name: ${String(value)}`);
  return value;
}
