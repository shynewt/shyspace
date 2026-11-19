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
  Channel,
  Member,
  Message,
  MessageCursor,
  Page,
  Session,
  StorageSettings,
} from "../../../src/lib/api";
import { argument, type Kind, ownedId, randomHex, requireString } from "./helpers";
import { FakeInvites } from "./invites";
import { search } from "./search";

/** The fake bucket refuses this secret, like a provider rejecting bad keys. */
export const REJECTED_SECRET = "wrong-secret";

type StoredChannel = Omit<Channel, "can_delete" | "unread" | "last_read">;
type StoredMessage = Omit<Message, "author_name" | "author_is_bot" | "reactions" | "can_edit" | "can_delete">;
type StoredPage = Omit<Page, "can_delete"> & { doc: Y.Doc };
type StoredReaction = { message: string; author: string; emoji: string };

function compareMessages(left: StoredMessage, right: StoredMessage): number {
  return left.lamport - right.lamport || left.author.localeCompare(right.author);
}

function isAfter(message: StoredMessage, cursor: MessageCursor): boolean {
  return message.lamport > cursor.lamport || (message.lamport === cursor.lamport && message.author > cursor.author);
}

function isBefore(message: StoredMessage, cursor: MessageCursor): boolean {
  return message.lamport < cursor.lamport || (message.lamport === cursor.lamport && message.author < cursor.author);
}

export class FakeBackend {
  session: Session | null = null;
  private members: Member[] = [];
  private channels: StoredChannel[] = [];
  private messages: StoredMessage[] = [];
  private reactions: StoredReaction[] = [];
  private pages: StoredPage[] = [];
  private invites = new FakeInvites(() => this.notify({ kind: "invites" }));
  private storage: StorageSettings | null = null;
  private lamport = 0;
  /** Personal read positions, by channel. */
  private readMarkers = new Map<string, MessageCursor>();

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
    this.members.push({ id, name, is_bot: kind === "bot", is_admin: false, active: true });
    this.notify({ kind: "members" });
    return id;
  }

  memberNamed(name: string): string {
    const member = this.members.find((candidate) => candidate.name === name);
    if (!member) throw new Error(`fake backend: no member named ${name}`);
    return member.id;
  }

  receiveMessage(author: string, channelName: string, text: string): string {
    return this.writeMessage(author, this.channelNamed(channelName).id, text, null);
  }

  receiveJoinRequest(name: string, kind: Kind = { kind: "person" }): string {
    return this.invites.receiveRequest(name, kind);
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
    switch (command) {
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
      case "update_storage":
        return this.updateStorage(requireString(args, "region"), requireString(args, "accessKeyId"));
      case "channels":
        return this.channels.map((channel) => ({
          ...channel,
          can_delete: this.mayDelete(channel.id),
          unread: this.unreadIn(channel.id),
          last_read: this.readMarkers.get(channel.id) ?? null,
        }));
      case "mark_read":
        return this.markRead(requireString(args, "channel"), requireString(args, "message"));
      case "create_channel":
        return this.createChannel(requireString(args, "name"));
      case "rename_channel":
        return this.renameChannel(requireString(args, "channel"), requireString(args, "name"));
      case "delete_channel":
        return this.deleteChannel(requireString(args, "channel"));
      case "edit_message":
        return this.editMessage(requireString(args, "message"), requireString(args, "text"));
      case "delete_message":
        return this.deleteMessage(requireString(args, "message"));
      case "messages":
        return this.messagesIn(
          requireString(args, "channel"),
          argument(args, "before") as MessageCursor | null,
          argument(args, "limit") as number,
        );
      case "send_message": {
        const channel = requireString(args, "channel");
        const id = this.writeMessage(
          this.me(),
          channel,
          requireString(args, "text"),
          (argument(args, "replyTo") as string | null) ?? null,
        );
        this.markRead(channel, id);
        return id;
      }
      case "react":
        return this.react(
          requireString(args, "message"),
          requireString(args, "emoji"),
          argument(args, "active") === true,
        );
      case "pages":
        return this.pages.map(({ doc: _doc, ...page }) => ({
          ...page,
          parent: this.pages.some((candidate) => candidate.id === page.parent) ? page.parent : null,
          can_delete: this.mayDelete(page.id),
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
          messages: this.messages,
          channels: this.channels,
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
    this.members = [{ id: me, name, is_bot: false, is_admin: true, active: true }];
    this.storage = {
      endpoint: bucket.endpoint,
      bucket: bucket.bucket,
      region: bucket.region,
      access_key_id: bucket.access_key_id,
    };
    this.session = { workspace_name: workspaceName, me, is_admin: true };
    this.createChannel("general");
    return this.session;
  }

  private joinWorkspace(name: string): Session {
    const admin = randomHex(32);
    const me = randomHex(32);
    this.members = [
      { id: admin, name: "Ada", is_bot: false, is_admin: true, active: true },
      { id: me, name, is_bot: false, is_admin: false, active: true },
    ];
    this.channels = [{ id: ownedId(admin), name: "general" }];
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

  // ---- Chat -------------------------------------------------------------

  private createChannel(name: string): string {
    const id = ownedId(this.me());
    this.channels.push({ id, name });
    this.tick();
    this.notify({ kind: "channels" });
    return id;
  }

  private channelNamed(name: string): StoredChannel {
    const channel = this.channels.find((candidate) => candidate.name === name);
    if (!channel) throw new Error(`fake backend: no channel #${name}`);
    return channel;
  }

  private writeMessage(author: string, channel: string, text: string, replyTo: string | null): string {
    const id = ownedId(author);
    const created_ms = Date.now();
    this.messages.push({
      id,
      channel,
      author,
      text,
      reply_to: replyTo,
      created_ms,
      lamport: this.tick(),
      is_edited: false,
    });
    this.notify({ kind: "messages", channel });
    return id;
  }

  private messagesIn(channel: string, before: MessageCursor | null, limit: number): Message[] {
    const inChannel = this.messages
      .filter((message) => message.channel === channel && (!before || isBefore(message, before)))
      .sort(compareMessages);
    return inChannel.slice(-limit).map((message) => this.present(message));
  }

  private present(message: StoredMessage): Message {
    const author = this.members.find((member) => member.id === message.author);
    const counts = new Map<string, { count: number; mine: boolean }>();
    for (const reaction of this.reactions.filter((candidate) => candidate.message === message.id)) {
      const entry = counts.get(reaction.emoji) ?? { count: 0, mine: false };
      entry.count += 1;
      entry.mine ||= reaction.author === this.me();
      counts.set(reaction.emoji, entry);
    }
    return {
      ...message,
      author_name: author?.name ?? "",
      author_is_bot: author?.is_bot ?? false,
      can_edit: message.author === this.me(),
      can_delete: this.mayDelete(message.id),
      reactions: Array.from(counts, ([emoji, entry]) => ({ emoji, ...entry })),
    };
  }

  private react(message: string, emoji: string, active: boolean): void {
    const me = this.me();
    this.reactions = this.reactions.filter(
      (reaction) => !(reaction.message === message && reaction.author === me && reaction.emoji === emoji),
    );
    if (active) this.reactions.push({ message, author: me, emoji });
    this.tick();
    const channel = this.messages.find((candidate) => candidate.id === message)?.channel;
    if (channel) this.notify({ kind: "messages", channel });
  }

  private unreadIn(channel: string): number {
    const marker = this.readMarkers.get(channel);
    return this.messages.filter(
      (message) => message.channel === channel && message.author !== this.me() && (!marker || isAfter(message, marker)),
    ).length;
  }

  /** Forward only, like the core. */
  private markRead(channel: string, messageId: string): void {
    const message = this.messages.find((candidate) => candidate.id === messageId && candidate.channel === channel);
    if (!message) throw "no such message in this channel";
    const marker = this.readMarkers.get(channel);
    if (marker && !isAfter(message, marker)) return;
    this.readMarkers.set(channel, { lamport: message.lamport, author: message.author });
    this.notify({ kind: "channels" });
  }

  private renameChannel(id: string, name: string): void {
    const channel = this.channels.find((candidate) => candidate.id === id);
    if (!channel) throw new Error(`fake backend: no channel ${id}`);
    channel.name = name;
    this.tick();
    this.notify({ kind: "channels" });
  }

  private deleteChannel(id: string): void {
    this.requireMayDelete(id);
    this.channels = this.channels.filter((channel) => channel.id !== id);
    this.messages = this.messages.filter((message) => message.channel !== id);
    this.notify({ kind: "channels" });
  }

  private editMessage(id: string, text: string): void {
    const message = this.messages.find((candidate) => candidate.id === id);
    if (!message) throw new Error(`fake backend: no message ${id}`);
    if (message.author !== this.me()) throw "only its owner or the admin can do that";
    message.text = text;
    message.is_edited = true;
    this.tick();
    this.notify({ kind: "messages", channel: message.channel });
  }

  private deleteMessage(id: string): void {
    this.requireMayDelete(id);
    const message = this.messages.find((candidate) => candidate.id === id);
    this.messages = this.messages.filter((candidate) => candidate.id !== id);
    this.reactions = this.reactions.filter((reaction) => reaction.message !== id);
    if (message) this.notify({ kind: "messages", channel: message.channel });
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

  private notify(change: Change): void {
    void emit("change", change);
  }
}
