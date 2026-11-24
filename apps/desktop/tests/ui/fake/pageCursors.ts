/**
 * Who is on which page and where their cursors are, as the core would hear
 * them live from other members. Records what this member shares.
 */
import type { InvokeArgs } from "@tauri-apps/api/core";
import * as Y from "yjs";
import type { Change, CursorSelection, PagePresence } from "../../../src/lib/api";
import { argument, requireString } from "./helpers";

export const NOT_PAGE_CURSORS = Symbol("not a page cursor command");

export type PageCursorsContext = {
  notify: (change: Change) => void;
  docOf: (page: string) => Y.Doc;
};

type Heard = { selection: CursorSelection | null; movedMs: number };

export class FakePageCursors {
  private readonly context: PageCursorsContext;
  /** Page → member → their cursor. */
  private readonly onPage = new Map<string, Map<string, Heard>>();
  /** Pages this member joined, and whether they share their cursor there. */
  readonly joined = new Map<string, boolean>();
  /** Pages this member left, in order. */
  readonly left: string[] = [];
  /** What this member sent, newest last. */
  readonly sent: { page: string; selection: CursorSelection | null }[] = [];

  constructor(context: PageCursorsContext) {
    this.context = context;
  }

  answer(command: string, args: InvokeArgs | undefined): unknown {
    switch (command) {
      case "join_page":
        this.joined.set(requireString(args, "page"), argument(args, "share") === true);
        return null;
      case "leave_page": {
        const page = requireString(args, "page");
        this.joined.delete(page);
        this.left.push(page);
        return null;
      }
      case "move_cursor": {
        const page = requireString(args, "page");
        if (this.joined.get(page) !== true) throw new Error("not sharing a cursor on that page");
        this.sent.push({ page, selection: (argument(args, "selection") as CursorSelection | null) ?? null });
        return null;
      }
      case "page_presence":
        return this.presence(requireString(args, "page"));
      default:
        return NOT_PAGE_CURSORS;
    }
  }

  // ---- Acting for tests ---------------------------------------------------

  /** Another member opens the page; they're in its text nowhere yet. */
  arrive(member: string, page: string): void {
    this.membersOn(page).set(member, { selection: null, movedMs: Date.now() });
    this.context.notify({ kind: "page_presence", page });
  }

  leave(member: string, page: string): void {
    this.membersOn(page).delete(member);
    this.context.notify({ kind: "page_presence", page });
  }

  /** Another member's selection, from `anchor` to `head` in the page's text. */
  moveCursor(member: string, page: string, anchor: number, head: number = anchor): void {
    const text = this.context.docOf(page).getText("content");
    const encode = (index: number) =>
      Array.from(Y.encodeRelativePosition(Y.createRelativePositionFromTypeIndex(text, index)));
    this.membersOn(page).set(member, {
      selection: { anchor: encode(anchor), head: encode(head) },
      movedMs: Date.now(),
    });
    this.context.notify({ kind: "page_presence", page });
  }

  // ---- Inside -------------------------------------------------------------

  private membersOn(page: string): Map<string, Heard> {
    let members = this.onPage.get(page);
    if (!members) {
      members = new Map();
      this.onPage.set(page, members);
    }
    return members;
  }

  private presence(page: string): PagePresence {
    const members = this.membersOn(page);
    return {
      viewers: [...members.keys()],
      cursors: [...members].map(([member, heard]) => ({
        member,
        selection: heard.selection,
        idle_ms: Date.now() - heard.movedMs,
      })),
    };
  }
}
