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
  Invite,
  Member,
  Message,
  MessageCursor,
  Page,
  SearchHit,
  Session,
  StorageSettings,
} from "../../../src/lib/api";

const HIGHLIGHT_START = "\u0001";
const HIGHLIGHT_END = "\u0002";
const WEEK_MS = 7 * 24 * 60 * 60 * 1000;
/** The fake bucket refuses this secret, like a provider rejecting bad keys. */
export const REJECTED_SECRET = "wrong-secret";

type StoredMessage = Omit<Message, "author_name" | "author_is_bot" | "reactions">;
type StoredPage = Omit<Page, "updated_ms"> & { updated_ms: number; doc: Y.Doc };
type StoredReaction = { message: string; author: string; emoji: string };

function randomHex(bytes: number): string {
  const values = crypto.getRandomValues(new Uint8Array(bytes));
  return Array.from(values, (value) => value.toString(16).padStart(2, "0")).join("");
}

function compareMessages(left: StoredMessage, right: StoredMessage): number {
  return left.lamport - right.lamport || left.author.localeCompare(right.author);
}

function isBefore(message: StoredMessage, cursor: MessageCursor): boolean {
  return message.lamport < cursor.lamport || (message.lamport === cursor.lamport && message.author < cursor.author);
}

function argument(args: InvokeArgs | undefined, name: string): unknown {
  if (!args || Array.isArray(args) || args instanceof ArrayBuffer || args instanceof Uint8Array) return undefined;
  return args[name];
}

function requireString(args: InvokeArgs | undefined, name: string): string {
  const value = argument(args, name);
  if (typeof value !== "string") throw new Error(`fake backend: missing string argument "${name}"`);
  return value;
}

type Kind = { kind: "person" | "bot" };

export class FakeBackend {
  session: Session | null = null;
  private members: Member[] = [];
  private channels: Channel[] = [];
  private messages: StoredMessage[] = [];
  private reactions: StoredReaction[] = [];
  private pages: StoredPage[] = [];
  private invites: Invite[] = [];
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

  // ---- Acting as other members, for tests --------------------------------

  addMember(name: string, { kind }: Kind = { kind: "person" }): string {
    const id = randomHex(32);
    this.members.push({ id, name, is_bot: kind === "bot", is_admin: false, active: true });
    this.notify({ kind: "members" });
    return id;
  }

  receiveMessage(author: string, channelName: string, text: string): string {
    return this.writeMessage(author, this.channelNamed(channelName).id, text, null);
  }

  receiveJoinRequest(name: string, { kind }: Kind = { kind: "person" }): string {
    const invite = this.invites.find((candidate) => candidate.state === "open") ?? this.newInvite("");
    invite.state = "requested";
    invite.requester_id = randomHex(32);
    invite.requester_name = name;
    invite.requester_is_bot = kind === "bot";
    invite.requester_safety_code = "1234 5678 9012 3456 7890";
    this.notify({ kind: "invites" });
    return invite.token;
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
        return [...this.channels].sort((left, right) => left.name.localeCompare(right.name));
      case "create_channel":
        return this.createChannel(requireString(args, "name"));
      case "messages":
        return this.messagesIn(
          requireString(args, "channel"),
          argument(args, "before") as MessageCursor | null,
          argument(args, "limit") as number,
        );
      case "send_message":
        return this.writeMessage(
          this.me(),
          requireString(args, "channel"),
          requireString(args, "text"),
          (argument(args, "replyTo") as string | null) ?? null,
        );
      case "react":
        return this.react(
          requireString(args, "message"),
          requireString(args, "emoji"),
          argument(args, "active") === true,
        );
      case "pages":
        return this.pages.map(({ doc: _doc, ...page }) => page);
      case "create_page":
        return this.createPage(requireString(args, "title"), (argument(args, "parent") as string | null) ?? null);
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
        return this.invites;
      case "create_invite":
        return `shy2_${this.newInvite(requireString(args, "label")).token}`;
      case "approve_invite":
        return this.approveInvite(requireString(args, "token"));
      case "reject_invite":
        return this.setInviteState(requireString(args, "token"), "rejected");
      case "revoke_invite":
        return this.setInviteState(requireString(args, "token"), "revoked");
      case "search":
        return this.search(requireString(args, "query"));
      case "status":
        return { peers: 1, last_sync_ms: Date.now(), error: null, removed: false };
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
    this.channels = [{ id: randomHex(24), name: "general" }];
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
    const id = randomHex(24);
    this.channels.push({ id, name });
    this.tick();
    this.notify({ kind: "channels" });
    return id;
  }

  private channelNamed(name: string): Channel {
    const channel = this.channels.find((candidate) => candidate.name === name);
    if (!channel) throw new Error(`fake backend: no channel #${name}`);
    return channel;
  }

  private writeMessage(author: string, channel: string, text: string, replyTo: string | null): string {
    const id = randomHex(24);
    this.messages.push({ id, channel, author, text, reply_to: replyTo, created_ms: Date.now(), lamport: this.tick() });
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

  // ---- Pages ------------------------------------------------------------

  private createPage(title: string, parent: string | null): string {
    const id = randomHex(24);
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

  // ---- Members and invites ----------------------------------------------

  private removeMember(id: string): void {
    const member = this.members.find((candidate) => candidate.id === id);
    if (!member) throw new Error(`fake backend: no member ${id}`);
    member.active = false;
    this.notify({ kind: "members" });
  }

  private newInvite(label: string): Invite {
    const now = Date.now();
    const invite: Invite = {
      token: randomHex(16),
      label,
      created_ms: now,
      expires_ms: now + WEEK_MS,
      state: "open",
      requester_id: null,
      requester_name: null,
      requester_is_bot: false,
      requester_safety_code: null,
    };
    this.invites.push(invite);
    this.notify({ kind: "invites" });
    return invite;
  }

  private invite(token: string): Invite {
    const invite = this.invites.find((candidate) => candidate.token === token);
    if (!invite) throw new Error(`fake backend: no invite ${token}`);
    return invite;
  }

  private approveInvite(token: string): void {
    const invite = this.invite(token);
    if (invite.state !== "requested" || !invite.requester_id) throw "this invite has no request to approve";
    invite.state = "accepted";
    this.members.push({
      id: invite.requester_id,
      name: invite.requester_name ?? "",
      is_bot: invite.requester_is_bot,
      is_admin: false,
      active: true,
    });
    this.notify({ kind: "invites" });
    this.notify({ kind: "members" });
  }

  private setInviteState(token: string, state: "rejected" | "revoked"): void {
    this.invite(token).state = state;
    this.notify({ kind: "invites" });
  }

  // ---- Search -----------------------------------------------------------

  private search(query: string): SearchHit[] {
    const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (terms.length === 0) return [];
    const matches = (text: string) =>
      terms.every((term) =>
        text
          .toLowerCase()
          .split(/\W+/)
          .some((word) => word.startsWith(term)),
      );
    const hits: SearchHit[] = [];
    for (const message of this.messages) {
      if (!matches(message.text)) continue;
      const channel = this.channels.find((candidate) => candidate.id === message.channel);
      hits.push({
        kind: "message",
        id: message.id,
        title: channel?.name ?? "",
        snippet: highlight(message.text, terms),
        channel: message.channel,
      });
    }
    for (const page of this.pages) {
      const body = page.doc.getText("content").toString();
      if (!matches(`${page.title} ${body}`)) continue;
      hits.push({ kind: "page", id: page.id, title: page.title, snippet: highlight(body, terms), channel: null });
    }
    return hits;
  }

  // ---- Plumbing ---------------------------------------------------------

  private me(): string {
    if (!this.session) throw new Error("fake backend: no workspace is open");
    return this.session.me;
  }

  private tick(): number {
    this.lamport += 1;
    return this.lamport;
  }

  private notify(change: Change): void {
    void emit("change", change);
  }
}

function highlight(text: string, terms: string[]): string {
  return text.replace(/\w+/g, (word) =>
    terms.some((term) => word.toLowerCase().startsWith(term)) ? `${HIGHLIGHT_START}${word}${HIGHLIGHT_END}` : word,
  );
}
