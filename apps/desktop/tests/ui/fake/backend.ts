/**
 * An in-memory stand-in for the Rust side of the desktop app, answering the
 * same `invoke` commands and emitting the same `change` events. It mirrors
 * the behavior the UI relies on (ordering, paging, which events fire), not
 * the crypto or sync underneath; those are covered by the Rust tests.
 */

import type { InvokeArgs } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import * as Y from "yjs";
import type {
  BucketConfig,
  Change,
  LaterItem,
  Member,
  Page,
  ProtectionCheck,
  SectionSort,
  Session,
  StorageProtection,
  StorageSettings,
} from "../../../src/lib/api";
import { FakeChat, NOT_CHAT } from "./chat";
import { FakeDrafts } from "./drafts";
import { argument, type Kind, ownedId, randomHex, requireNotifyLevel, requireString } from "./helpers";
import { FakeInvites } from "./invites";
import { FakeLater } from "./later";
import { search } from "./search";
import { FakeSections } from "./sections";

/** The fake bucket refuses this secret, like a provider rejecting bad keys. */
export const REJECTED_SECRET = "wrong-secret";

type StoredPage = Omit<Page, "can_delete" | "is_saved"> & { doc: Y.Doc };

export class FakeBackend {
  session: Session | null = null;
  private members: Member[] = [];
  private pages: StoredPage[] = [];
  private chat = new FakeChat({
    me: () => this.me(),
    isAdmin: () => Boolean(this.session?.is_admin),
    members: () => this.members,
    tick: () => this.tick(),
    notify: (change) => this.notify(change),
    savedMark: (item) => this.later.mark(item),
  });
  private later = new FakeLater();
  private sections = new FakeSections();
  private drafts = new FakeDrafts();
  private invites = new FakeInvites(() => this.notify({ kind: "invites" }));
  private storage: StorageSettings | null = null;
  private lamport = 0;

  /** Starts as the admin of a workspace with #general, as `create` would. */
  static withWorkspace(workspaceName: string, myName: string): FakeBackend {
    const backend = new FakeBackend();
    backend.createWorkspace(workspaceName, myName, {
      endpoint: "https://s3.example.com",
      region: "auto",
      bucket: "team",
      access_key_id: "AKIDEXAMPLE",
      secret_access_key: "secret",
    });
    return backend;
  }

  /** Starts as a regular member who joined Ada's workspace "Team". */
  static asMember(myName: string): FakeBackend {
    const backend = new FakeBackend();
    backend.joinWorkspace(myName);
    return backend;
  }

  // ---- Acting as other members, for tests --------------------------------

  addMember(name: string, { kind }: Kind = { kind: "person" }): string {
    const id = randomHex(32);
    this.members.push({
      id,
      name,
      is_bot: kind === "bot",
      is_admin: false,
      active: true,
      status: null,
      presence: "unreachable",
    });
    this.notify({ kind: "members" });
    return id;
  }

  memberNamed(name: string): string {
    const member = this.members.find((candidate) => candidate.name === name);
    if (!member) throw new Error(`fake backend: no member named ${name}`);
    return member.id;
  }

  /** What the app told the core about this device's presence, in order. */
  readonly ownPresenceLog: string[] = [];
  private isAppearingOffline = false;

  private storageProtection: ProtectionCheck = { state: "unknown", checked_ms: null };

  /** Sets what checking the bucket finds, as a provider's settings would. */
  setStorageProtection(state: StorageProtection): void {
    this.storageProtection = { state, checked_ms: Date.now() };
    this.notify({ kind: "storage_protection" });
  }

  /** Sets another member's presence, as their heartbeats would. */
  setPresence(member: string, presence: "online" | "away" | "unreachable"): void {
    const found = this.members.find((candidate) => candidate.id === member);
    if (!found) throw new Error(`fake backend: no member ${member}`);
    found.presence = presence;
    this.notify({ kind: "members" });
  }

  /** Desktop notifications the app asked to show. */
  readonly alertsShown: { title: string; body: string }[] = [];

  receiveMessage(author: string, channelName: string, text: string): string {
    return this.chat.receiveMessage(author, channelName, text);
  }

  receiveReply(author: string, root: string, text: string): string {
    return this.chat.receiveReply(author, root, text);
  }

  messageWithText(text: string): string {
    return this.chat.messageWithText(text);
  }

  receiveJoinRequest(name: string, kind: Kind = { kind: "person" }): string {
    return this.invites.receiveRequest(name, kind);
  }

  /** Rebuilds a page from scratch with `text`, as the core does after undoing a removed member's edits. */
  rebuildPage(title: string, text: string): void {
    const page = this.pages.find((candidate) => candidate.title === title);
    if (!page) throw new Error(`fake backend: no page titled "${title}"`);
    page.doc = new Y.Doc();
    page.doc.getText("content").insert(0, text);
    this.notify({ kind: "page_reset", page: page.id });
  }

  pageText(title: string): string {
    const page = this.pages.find((candidate) => candidate.title === title);
    if (!page) throw new Error(`fake backend: no page titled "${title}"`);
    return page.doc.getText("content").toString();
  }

  // ---- The command surface ----------------------------------------------

  /** Answers a command with a copy, as crossing the real IPC boundary would. */
  handle(command: string, args: InvokeArgs | undefined): unknown {
    return structuredClone(this.answer(command, args));
  }

  private answer(command: string, args: InvokeArgs | undefined): unknown {
    const chatAnswer = this.chat.answer(command, args);
    if (command === "send_message" && typeof chatAnswer === "string") {
      const target = this.chat.draftTargetOf(chatAnswer);
      if (target) this.drafts.set(target, "");
      this.notify({ kind: "drafts" });
    }
    if (chatAnswer !== NOT_CHAT) return chatAnswer;
    switch (command) {
      case "sidebar_sections":
        return this.sections.list(
          this.chat.channels.map((channel) => ({
            id: channel.id,
            name: channel.name,
            lastActivity: Math.max(
              0,
              ...this.chat.messages
                .filter((message) => message.channel === channel.id)
                .map((message) => message.created_ms),
            ),
          })),
        );
      case "create_section": {
        const id = this.sections.create(requireString(args, "name"));
        this.notify({ kind: "channels" });
        return id;
      }
      case "rename_section":
        this.sections.rename(requireString(args, "section"), requireString(args, "name"));
        return this.notify({ kind: "channels" });
      case "delete_section":
        this.sections.delete(requireString(args, "section"));
        return this.notify({ kind: "channels" });
      case "set_section_sort":
        this.sections.setSort(requireString(args, "section"), requireString(args, "sort") as SectionSort);
        return this.notify({ kind: "channels" });
      case "set_section_collapsed":
        this.sections.setCollapsed(requireString(args, "section"), argument(args, "collapsed") === true);
        return this.notify({ kind: "channels" });
      case "move_channel":
        this.sections.move(requireString(args, "channel"), (argument(args, "section") as string | null) ?? null);
        return this.notify({ kind: "channels" });
      case "set_status": {
        const me = this.myMember();
        me.status = {
          emoji: requireString(args, "emoji"),
          text: requireString(args, "text"),
          until_ms: (argument(args, "untilMs") as number | null) ?? null,
          alerts: requireNotifyLevel(args, "alerts"),
        };
        return this.notify({ kind: "members" });
      }
      case "clear_status":
        this.myMember().status = null;
        return this.notify({ kind: "members" });
      case "set_away":
        this.ownPresenceLog.push(argument(args, "away") === true ? "away" : "back");
        return this.refreshOwnPresence();
      case "appear_offline":
        return this.isAppearingOffline;
      case "set_appear_offline":
        this.isAppearingOffline = argument(args, "offline") === true;
        this.ownPresenceLog.push(this.isAppearingOffline ? "offline" : "visible");
        return this.refreshOwnPresence();
      case "set_profile": {
        const me = this.members.find((member) => member.id === this.me());
        if (!me) throw new Error("fake backend: no member for me");
        me.name = requireString(args, "name").trim();
        return this.notify({ kind: "members" });
      }
      case "save_for_later":
        this.later.save(requireString(args, "item"), (argument(args, "remindMs") as number | null) ?? null);
        return this.notifyLater();
      case "remove_from_later":
        this.later.remove(requireString(args, "item"));
        return this.notifyLater();
      case "mark_later_done":
        this.later.setDone(requireString(args, "item"), argument(args, "done") === true);
        return this.notifyLater();
      case "later":
        return this.laterItems();
      case "take_due_reminders": {
        const due = this.later.takeDue(Date.now());
        if (due.length > 0) this.notify({ kind: "later" });
        return this.laterItems().filter((item) => due.includes(item.id));
      }
      case "set_draft":
        this.drafts.set(requireString(args, "target"), requireString(args, "text"));
        return this.notify({ kind: "drafts" });
      case "drafts":
        return this.drafts.list((target) => this.chat.channelOf(target));
      case "session":
        return this.session;
      case "create_workspace":
        return this.createWorkspace(
          requireString(args, "workspaceName"),
          requireString(args, "name"),
          argument(args, "bucket") as BucketConfig,
        );
      case "join_workspace":
        return this.joinWorkspace(requireString(args, "name"));
      case "test_bucket":
        return this.testBucket(argument(args, "bucket") as BucketConfig);
      case "safety_code":
        return "8506 3956 2869 2874 2976";
      case "recovery_kit":
        return `shyrec1_${randomHex(48)}`;
      case "storage_settings":
        return this.requireStorage();
      case "storage_protection":
        return this.storageProtection;
      case "check_storage_protection":
        this.storageProtection = { ...this.storageProtection, checked_ms: Date.now() };
        this.notify({ kind: "storage_protection" });
        return this.storageProtection;
      case "update_storage":
        return this.updateStorage(requireString(args, "region"), requireString(args, "accessKeyId"));
      case "show_alert":
        this.alertsShown.push({ title: requireString(args, "title"), body: requireString(args, "body") });
        return null;
      case "pages":
        return this.pages.map(({ doc: _doc, ...page }) => ({
          ...page,
          parent: this.pages.some((candidate) => candidate.id === page.parent) ? page.parent : null,
          can_delete: this.mayDelete(page.id),
          is_saved: this.later.has(page.id),
        }));
      case "create_page":
        return this.createPage(requireString(args, "title"), (argument(args, "parent") as string | null) ?? null);
      case "delete_page":
        return this.deletePage(requireString(args, "page"));
      case "rename_page":
        return this.renamePage(requireString(args, "page"), requireString(args, "title"));
      case "page_state":
        return Y.encodeStateAsUpdate(this.page(requireString(args, "page")).doc);
      case "apply_doc_update":
        return this.applyDocUpdate(requireString(args, "page"), argument(args, "update") as number[]);
      case "members":
        return this.members;
      case "remove_member":
        return this.removeMember(requireString(args, "member"));
      case "invites":
        return this.invites.list();
      case "create_invite":
        return `shy2_${this.invites.create(requireString(args, "label")).token}`;
      case "approve_invite":
        return this.approveInvite(requireString(args, "token"));
      case "reject_invite":
        return this.invites.setState(requireString(args, "token"), "rejected");
      case "revoke_invite":
        return this.invites.setState(requireString(args, "token"), "revoked");
      case "search":
        return search(requireString(args, "query"), {
          messages: this.chat.messages,
          channels: this.chat.channels,
          pages: this.pages,
        });
      case "status":
        return { peers: 1, last_sync_ms: Date.now(), error: null, removed: false, needs_update: false };
      default:
        throw new Error(`fake backend: unknown command "${command}"`);
    }
  }

  // ---- Workspace --------------------------------------------------------

  private createWorkspace(workspaceName: string, name: string, bucket: BucketConfig): Session {
    const me = randomHex(32);
    this.members = [{ id: me, name, is_bot: false, is_admin: true, active: true, status: null, presence: "online" }];
    this.storage = {
      endpoint: bucket.endpoint,
      bucket: bucket.bucket,
      region: bucket.region,
      access_key_id: bucket.access_key_id,
    };
    this.session = { workspace_name: workspaceName, me, is_admin: true };
    this.chat.createChannel("general");
    return this.session;
  }

  private joinWorkspace(name: string): Session {
    const admin = randomHex(32);
    const me = randomHex(32);
    this.members = [
      { id: admin, name: "Ada", is_bot: false, is_admin: true, active: true, status: null, presence: "unreachable" },
      { id: me, name, is_bot: false, is_admin: false, active: true, status: null, presence: "online" },
    ];
    this.chat.addChannel(ownedId(admin), "general");
    this.session = { workspace_name: "Team", me, is_admin: false };
    return this.session;
  }

  private testBucket(bucket: BucketConfig): void {
    if (bucket.secret_access_key === REJECTED_SECRET) {
      throw "the bucket refused these keys: 403 Forbidden: The request signature we calculated does not match";
    }
  }

  private requireStorage(): StorageSettings {
    if (!this.storage) throw new Error("fake backend: no workspace");
    return this.storage;
  }

  private updateStorage(region: string, accessKeyId: string): string | null {
    this.storage = { ...this.requireStorage(), region, access_key_id: accessKeyId };
    this.notify({ kind: "storage" });
    return `shyrec1_${randomHex(48)}`;
  }

  // ---- Pages ------------------------------------------------------------

  private createPage(title: string, parent: string | null): string {
    const id = ownedId(this.me());
    this.pages.push({ id, parent, title, updated_ms: Date.now(), doc: new Y.Doc() });
    this.tick();
    this.notify({ kind: "pages" });
    return id;
  }

  private page(id: string): StoredPage {
    const page = this.pages.find((candidate) => candidate.id === id);
    if (!page) throw new Error(`fake backend: no page ${id}`);
    return page;
  }

  private renamePage(id: string, title: string): void {
    const page = this.page(id);
    page.title = title;
    page.updated_ms = Date.now();
    this.tick();
    this.notify({ kind: "pages" });
  }

  private applyDocUpdate(id: string, update: number[]): void {
    const page = this.page(id);
    Y.applyUpdate(page.doc, Uint8Array.from(update));
    page.updated_ms = Date.now();
    this.tick();
    this.notify({ kind: "doc", page: id, update });
  }

  private deletePage(id: string): void {
    this.requireMayDelete(id);
    this.pages = this.pages.filter((page) => page.id !== id);
    this.notify({ kind: "pages" });
  }

  // ---- Members and invites ----------------------------------------------

  private removeMember(id: string): void {
    const member = this.members.find((candidate) => candidate.id === id);
    if (!member) throw new Error(`fake backend: no member ${id}`);
    member.active = false;
    this.notify({ kind: "members" });
  }

  private approveInvite(token: string): void {
    this.members.push(this.invites.approve(token));
    this.notify({ kind: "members" });
  }

  // ---- Plumbing ---------------------------------------------------------

  private refreshOwnPresence(): void {
    const isAway = this.ownPresenceLog.findLast((entry) => entry === "away" || entry === "back") === "away";
    this.myMember().presence = this.isAppearingOffline ? "unreachable" : isAway ? "away" : "online";
    this.notify({ kind: "members" });
  }

  private myMember(): Member {
    const me = this.members.find((member) => member.id === this.me());
    if (!me) throw new Error("fake backend: no member for me");
    return me;
  }

  private me(): string {
    if (!this.session) throw new Error("fake backend: no workspace is open");
    return this.session.me;
  }

  private mayDelete(id: string): boolean {
    return Boolean(this.session?.is_admin) || id.startsWith(this.me().slice(0, 32));
  }

  private requireMayDelete(id: string): void {
    if (!this.mayDelete(id)) throw "only its owner or the admin can do that";
  }

  private tick(): number {
    this.lamport += 1;
    return this.lamport;
  }

  private laterItems(): LaterItem[] {
    const pages = this.answer("pages", undefined) as Page[];
    return this.later.list((item) => ({
      message: this.chat.linkedMessage(item),
      page: pages.find((page) => page.id === item) ?? null,
    }));
  }

  private notifyLater(): void {
    this.notify({ kind: "later" });
    for (const channel of this.chat.channels) this.notify({ kind: "messages", channel: channel.id });
    this.notify({ kind: "pages" });
  }

  private notify(change: Change): void {
    void emit("change", change);
  }
}
