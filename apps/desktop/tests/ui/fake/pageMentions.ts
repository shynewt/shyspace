/**
 * Mentions of you in pages, in the fake: another member's edit that adds
 * one, the inbox entries it leaves (one per page and author), and the alert
 * the core raises for it.
 */
import type { InvokeArgs } from "@tauri-apps/api/core";
import * as Y from "yjs";
import type { Change, PageMention } from "../../../src/lib/api";
import { requireString } from "./helpers";

export const NOT_PAGE_MENTIONS = Symbol("not a page mention command");

export type PageMentionsContext = {
  notify: (change: Change) => void;
  /** The page's id, title and doc; throws if there's no such page. */
  pageTitled: (title: string) => { id: string; title: string; doc: Y.Doc };
  pageExists: (page: string) => boolean;
  me: () => string;
  myName: () => string;
  workspaceId: () => string;
  nameOf: (member: string) => string;
};

type Entry = PageMention & { isRead: boolean };

export class FakePageMentions {
  private readonly context: PageMentionsContext;
  private entries: Entry[] = [];

  constructor(context: PageMentionsContext) {
    this.context = context;
  }

  answer(command: string, args: InvokeArgs | undefined): unknown {
    switch (command) {
      case "mark_page_mentions_read":
        return this.markRead((entry) => entry.page === requireString(args, "page"));
      default:
        return NOT_PAGE_MENTIONS;
    }
  }

  /** Unread ones in pages that still exist, newest first. */
  unread(): PageMention[] {
    return this.entries
      .filter((entry) => !entry.isRead && this.context.pageExists(entry.page))
      .sort((left, right) => right.mentioned_ms - left.mentioned_ms)
      .map(({ isRead: _isRead, ...mention }) => mention);
  }

  markAllRead(): void {
    this.markRead(() => true);
  }

  /**
   * `author` adding `before`, a mention of you and `after` as a new
   * paragraph at the end of the page, as one update, as the core would
   * hear it. Alerts unless `isAlerting` is false (a status, or an old edit).
   */
  receive(title: string, author: string, before: string, after: string, isAlerting = true): void {
    const page = this.context.pageTitled(title);
    const theirs = new Y.Doc();
    Y.applyUpdate(theirs, Y.encodeStateAsUpdate(page.doc));
    const stateBefore = Y.encodeStateVector(theirs);
    const text = theirs.getText("content");
    const start = text.length === 0 ? 0 : text.length + 2;
    const link = `[@${this.context.myName()}](shy://${this.context.workspaceId()}/member/${this.context.me()})`;
    text.insert(text.length, `${start === 0 ? "" : "\n\n"}${before}${link}${after}`);
    const position = Array.from(
      Y.encodeRelativePosition(Y.createRelativePositionFromTypeIndex(text, start + before.length)),
    );
    const update = Y.encodeStateAsUpdate(theirs, stateBefore);
    Y.applyUpdate(page.doc, update);
    this.context.notify({ kind: "doc", page: page.id, update: Array.from(update) });

    const preview = `${before}@${this.context.myName()}${after}`;
    const mention: Entry = {
      page: page.id,
      page_title: page.title,
      author,
      preview,
      position,
      mentioned_ms: Date.now(),
      isRead: false,
    };
    this.entries = [...this.entries.filter((entry) => entry.page !== page.id || entry.author !== author), mention];
    this.context.notify({ kind: "page_mentions" });
    if (!isAlerting) return;
    this.context.notify({
      kind: "page_alert",
      page: page.id,
      page_title: page.title,
      author,
      author_name: this.context.nameOf(author),
      preview,
      position,
    });
  }

  private markRead(matches: (entry: Entry) => boolean): null {
    const unread = this.entries.filter((entry) => !entry.isRead && matches(entry));
    for (const entry of unread) entry.isRead = true;
    // As in the core: only a change is announced.
    if (unread.length > 0) this.context.notify({ kind: "page_mentions" });
    return null;
  }
}
