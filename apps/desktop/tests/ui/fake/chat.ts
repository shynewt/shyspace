/** The chat side of the fake: channels, messages, threads and personal state. */

import type {
  Change,
  Channel,
  FileInfo,
  Inbox,
  InboxItem,
  LinkedMessage,
  Member,
  Message,
  MessageCursor,
  NotifyLevel,
  SavedMark,
} from "../../../src/lib/api";
import { mentions } from "../../../src/lib/mentions";
import { argument, ownedId, requireString } from "./helpers";
import type { PrivateView } from "./private";
import { followsByDefault, isInChannel, repliesOf, rootOf } from "./threads";
import { type LinkNames, visibleText } from "./visibleText";

/** What the chat needs from the rest of the fake. */
export type ChatContext = {
  me: () => string;
  isAdmin: () => boolean;
  members: () => readonly Member[];
  tick: () => number;
  notify: (change: Change) => void;
  savedMark: (item: string) => SavedMark | null;
  /** How a private channel or DM shows, or null for a public one. */
  privateView: (channel: string) => PrivateView | null;
  filesOf: (message: string) => FileInfo[];
  /** A message was deleted, with its files. */
  onMessageDeleted: (message: string) => void;
  pageTitle: (id: string) => string | undefined;
};

/** Returned by `answer` for commands that aren't about chat. */
export const NOT_CHAT = Symbol("not a chat command");

type StoredChannel = Pick<Channel, "id" | "name">;
type StoredMessage = Omit<
  Message,
  | "author_name"
  | "author_is_bot"
  | "reactions"
  | "can_edit"
  | "can_delete"
  | "mentions_me"
  | "saved"
  | "reply_count"
  | "last_reply_ms"
  | "recent_reply_authors"
  | "last_reply_author"
  | "last_reply_preview"
  | "is_following"
  | "unread_replies"
  | "files"
>;
type StoredReaction = { message: string; author: string; emoji: string };

const REPLY_PREVIEW_CHARS = 100;

/** Like the core: visible text on one line, links named, cut on a character boundary. */
function replyPreview(text: string, names: LinkNames): string {
  const characters = Array.from(visibleText(text, names));
  if (characters.length <= REPLY_PREVIEW_CHARS) return characters.join("");
  return `${characters
    .slice(0, REPLY_PREVIEW_CHARS - 1)
    .join("")
    .trimEnd()}…`;
}

function recentAuthors(replies: readonly StoredMessage[]): string[] {
  return [...new Set(replies.map((reply) => reply.author).reverse())].slice(0, 3);
}

function compareMessages(left: StoredMessage, right: StoredMessage): number {
  return left.lamport - right.lamport || left.author.localeCompare(right.author);
}

function isAfter(message: StoredMessage, cursor: MessageCursor): boolean {
  return message.lamport > cursor.lamport || (message.lamport === cursor.lamport && message.author > cursor.author);
}

function isBefore(message: StoredMessage, cursor: MessageCursor): boolean {
  return message.lamport < cursor.lamport || (message.lamport === cursor.lamport && message.author < cursor.author);
}

export class FakeChat {
  channels: StoredChannel[] = [];
  messages: StoredMessage[] = [];
  private reactions: StoredReaction[] = [];
  /** Personal read positions and alert levels, by channel. */
  private readMarkers = new Map<string, MessageCursor>();
  private notifyLevels = new Map<string, NotifyLevel>();
  private followChoices = new Map<string, boolean>();
  private readonly context: ChatContext;

  constructor(context: ChatContext) {
    this.context = context;
  }

  answer(command: string, args: Parameters<typeof argument>[0]): unknown {
    switch (command) {
      case "channels":
        return this.channelViews();
      case "inbox":
        return this.inbox();
      case "linked_message":
        return this.linkedMessage(requireString(args, "id"));
      case "mark_all_read":
        return this.markAllRead();
      case "set_notify_level":
        this.notifyLevels.set(requireString(args, "channel"), requireString(args, "level") as NotifyLevel);
        return this.context.notify({ kind: "channels" });
      case "thread": {
        const root = requireString(args, "root");
        const first = this.messages.find((message) => message.id === root);
        if (!first) throw new Error(`fake backend: no message ${root}`);
        return [first, ...repliesOf(this.messages, root).sort(compareMessages)].map((message) => this.present(message));
      }
      case "set_following": {
        const root = requireString(args, "root");
        this.followChoices.set(root, argument(args, "following") === true);
        const channel = this.messages.find((message) => message.id === root)?.channel;
        if (channel) this.context.notify({ kind: "messages", channel });
        return null;
      }
      case "mark_thread_read":
        return this.markThreadRead(requireString(args, "root"), requireString(args, "message"));
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
        if (this.context.privateView(channel)?.is_member === false) {
          throw new Error("you aren't a member of this channel");
        }
        const id = this.writeMessage(
          this.context.me(),
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
      default:
        return NOT_CHAT;
    }
  }

  // ---- Acting as other members, for tests --------------------------------

  receiveMessage(author: string, channelName: string, text: string): string {
    return this.receiveMessageIn(author, this.channelNamed(channelName).id, text);
  }

  /** Like `receiveMessage`, for channels named by id, like DMs (which have no name). */
  receiveMessageIn(author: string, channelId: string, text: string): string {
    const channel = this.channels.find((candidate) => candidate.id === channelId);
    if (!channel) throw new Error(`fake backend: no channel ${channelId}`);
    const id = this.writeMessage(author, channel.id, text, null);
    const level = this.notifyLevels.get(channel.id) ?? "all";
    if (level === "all" || (level === "mentions" && mentions(text, this.context.me()))) {
      const member = this.context.members().find((candidate) => candidate.id === author);
      this.context.notify({
        kind: "alert",
        channel: channel.id,
        channel_name: channel.name,
        message: id,
        author,
        author_name: member?.name ?? "",
        preview: visibleText(text, this.linkNames()),
      });
    }
    return id;
  }

  receiveReply(author: string, root: string, text: string): string {
    const channel = this.messages.find((message) => message.id === root)?.channel;
    if (!channel) throw new Error(`fake backend: no message ${root}`);
    return this.writeMessage(author, channel, text, root);
  }

  receiveReaction(author: string, message: string, emoji: string): void {
    const channel = this.messages.find((candidate) => candidate.id === message)?.channel;
    if (!channel) throw new Error(`fake backend: no message ${message}`);
    this.reactions.push({ message, author, emoji });
    this.context.tick();
    this.context.notify({ kind: "messages", channel });
  }

  /** The id of the latest message with this text. */
  messageWithText(text: string): string {
    const message = this.messages.findLast((candidate) => candidate.text === text);
    if (!message) throw new Error(`fake backend: no message saying ${text}`);
    return message.id;
  }

  linkedMessage(id: string): LinkedMessage | null {
    const message = this.messages.find((candidate) => candidate.id === id);
    if (!message) return null;
    const root = rootOf(this.messages, id);
    return { message: this.present(message), thread_root: root === id ? null : root };
  }

  /** The channel a draft target is in: itself, or a thread's channel. */
  channelOf(target: string): string | null {
    if (this.channels.some((channel) => channel.id === target)) return target;
    return this.messages.find((message) => message.id === target)?.channel ?? null;
  }

  /** Where a sent message's draft lived: its thread's first message, or its channel. */
  draftTargetOf(messageId: string): string | null {
    const message = this.messages.find((candidate) => candidate.id === messageId);
    if (!message) return null;
    const root = rootOf(this.messages, messageId);
    return root && root !== messageId ? root : message.channel;
  }

  /** A channel as a fresh workspace has it. */
  addChannel(id: string, name: string): void {
    this.channels.push({ id, name });
  }

  // ---- Chat -------------------------------------------------------------

  createChannel(name: string): string {
    const id = ownedId(this.context.me());
    this.listChannel(id, name);
    return id;
  }

  /** Lists a channel that was just made, and tells the app. */
  listChannel(id: string, name: string): void {
    this.channels.push({ id, name });
    this.context.tick();
    this.context.notify({ kind: "channels" });
  }

  channelNamed(name: string): StoredChannel {
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
      lamport: this.context.tick(),
      is_edited: false,
    });
    this.context.notify({ kind: "messages", channel });
    return id;
  }

  private messagesIn(channel: string, before: MessageCursor | null, limit: number): Message[] {
    const inChannel = this.messages
      .filter(
        (message) =>
          message.channel === channel && isInChannel(this.messages, message) && (!before || isBefore(message, before)),
      )
      .sort(compareMessages);
    return inChannel.slice(-limit).map((message) => this.present(message));
  }

  private present(message: StoredMessage): Message {
    const author = this.context.members().find((member) => member.id === message.author);
    const counts = new Map<string, { count: number; mine: boolean }>();
    for (const reaction of this.reactions.filter((candidate) => candidate.message === message.id)) {
      const entry = counts.get(reaction.emoji) ?? { count: 0, mine: false };
      entry.count += 1;
      entry.mine ||= reaction.author === this.context.me();
      counts.set(reaction.emoji, entry);
    }
    return {
      ...message,
      author_name: author?.name ?? "",
      author_is_bot: author?.is_bot ?? false,
      can_edit: message.author === this.context.me(),
      can_delete: this.mayDelete(message.id),
      mentions_me: message.author !== this.context.me() && mentions(message.text, this.context.me()),
      saved: this.context.savedMark(message.id),
      ...this.threadSummary(message),
      reactions: Array.from(counts, ([emoji, entry]) => ({ emoji, ...entry })),
      files: this.context.filesOf(message.id),
    };
  }

  private linkNames(): LinkNames {
    return {
      member: (id) => this.context.members().find((member) => member.id === id)?.name,
      page: this.context.pageTitle,
    };
  }

  private threadSummary(message: StoredMessage) {
    const replies = repliesOf(this.messages, message.id).sort(compareMessages);
    const isFollowing = this.followChoices.get(message.id) ?? followsByDefault(this.context.me(), message, replies);
    const read = this.readMarkers.get(message.id);
    const last = replies.at(-1);
    return {
      reply_count: replies.length,
      last_reply_ms: last?.created_ms ?? null,
      recent_reply_authors: recentAuthors(replies),
      last_reply_author: last?.author ?? null,
      last_reply_preview: last === undefined ? null : replyPreview(last.text, this.linkNames()),
      is_following: isFollowing,
      unread_replies: isFollowing
        ? replies.filter((reply) => reply.author !== this.context.me() && (!read || isAfter(reply, read))).length
        : 0,
    };
  }

  private markThreadRead(root: string, messageId: string): void {
    const message = this.messages.find((candidate) => candidate.id === messageId);
    if (!message || rootOf(this.messages, messageId) !== root) throw "that message isn't in this thread";
    const marker = this.readMarkers.get(root);
    if (marker && !isAfter(message, marker)) return;
    this.readMarkers.set(root, { lamport: message.lamport, author: message.author });
    this.context.notify({ kind: "messages", channel: message.channel });
  }

  private react(message: string, emoji: string, active: boolean): void {
    const me = this.context.me();
    this.reactions = this.reactions.filter(
      (reaction) => !(reaction.message === message && reaction.author === me && reaction.emoji === emoji),
    );
    if (active) this.reactions.push({ message, author: me, emoji });
    this.context.tick();
    const channel = this.messages.find((candidate) => candidate.id === message)?.channel;
    if (channel) this.context.notify({ kind: "messages", channel });
  }

  /** Like the core: a thread reply is also read once its thread is read past it. */
  private unreadIn(channel: string): StoredMessage[] {
    const marker = this.readMarkers.get(channel);
    return this.messages.filter(
      (message) =>
        message.channel === channel &&
        message.author !== this.context.me() &&
        (!marker || isAfter(message, marker)) &&
        !this.isReadInThread(message),
    );
  }

  private isReadInThread(message: StoredMessage): boolean {
    const root = rootOf(this.messages, message.id);
    if (isInChannel(this.messages, message) || root === null) return false;
    const threadMarker = this.readMarkers.get(root);
    return threadMarker !== undefined && !isAfter(message, threadMarker);
  }

  /** Forward only, like the core. */
  private markRead(channel: string, messageId: string): void {
    const message = this.messages.find((candidate) => candidate.id === messageId && candidate.channel === channel);
    if (!message) throw "no such message in this channel";
    const marker = this.readMarkers.get(channel);
    if (marker && !isAfter(message, marker)) return;
    this.readMarkers.set(channel, { lamport: message.lamport, author: message.author });
    this.context.notify({ kind: "channels" });
  }

  private renameChannel(id: string, name: string): void {
    const channel = this.channels.find((candidate) => candidate.id === id);
    if (!channel) throw new Error(`fake backend: no channel ${id}`);
    channel.name = name;
    this.context.tick();
    this.context.notify({ kind: "channels" });
  }

  private deleteChannel(id: string): void {
    this.requireMayDelete(id);
    this.channels = this.channels.filter((channel) => channel.id !== id);
    this.messages = this.messages.filter((message) => message.channel !== id);
    this.context.notify({ kind: "channels" });
  }

  private editMessage(id: string, text: string): void {
    const message = this.messages.find((candidate) => candidate.id === id);
    if (!message) throw new Error(`fake backend: no message ${id}`);
    if (message.author !== this.context.me()) throw "only its owner or the admin can do that";
    message.text = text;
    message.is_edited = true;
    this.context.tick();
    this.context.notify({ kind: "messages", channel: message.channel });
  }

  private deleteMessage(id: string): void {
    this.requireMayDelete(id);
    const message = this.messages.find((candidate) => candidate.id === id);
    this.messages = this.messages.filter((candidate) => candidate.id !== id);
    this.reactions = this.reactions.filter((reaction) => reaction.message !== id);
    this.context.onMessageDeleted(id);
    if (message) this.context.notify({ kind: "messages", channel: message.channel });
  }

  private channelViews(): Channel[] {
    return this.channels.map((channel) => ({
      ...channel,
      kind: "public" as const,
      dm_members: [],
      is_member: true,
      is_missing_key: false,
      can_delete: this.mayDelete(channel.id),
      ...this.context.privateView(channel.id),
      unread: this.unreadIn(channel.id).filter((message) => isInChannel(this.messages, message)).length,
      unread_mentions: this.unreadIn(channel.id).filter((message) => mentions(message.text, this.context.me())).length,
      last_read: this.readMarkers.get(channel.id) ?? null,
      notify: this.notifyLevels.get(channel.id) ?? "all",
    }));
  }

  /** Like the core: unread mentions, then new replies in followed threads. */
  private inbox(): Inbox {
    const me = this.context.me();
    const items: InboxItem[] = [];
    for (const channel of this.channels) {
      for (const message of this.unreadIn(channel.id).filter((unread) => mentions(unread.text, me))) {
        const root = rootOf(this.messages, message.id);
        items.push({
          message: this.present(message),
          channel_name: channel.name,
          thread_root: root === message.id ? null : root,
          reason: "mention",
        });
      }
      const roots = this.messages.filter(
        (message) => message.channel === channel.id && isInChannel(this.messages, message),
      );
      for (const root of roots) {
        const replies = repliesOf(this.messages, root.id);
        if (!(this.followChoices.get(root.id) ?? followsByDefault(me, root, replies))) continue;
        const read = this.readMarkers.get(root.id);
        for (const reply of replies) {
          const isNew = reply.author !== me && (!read || isAfter(reply, read));
          if (isNew && !items.some((item) => item.message.id === reply.id)) {
            items.push({
              message: this.present(reply),
              channel_name: channel.name,
              thread_root: root.id,
              reason: "thread_reply",
            });
          }
        }
      }
    }
    items.sort((left, right) => compareMessages(right.message, left.message));
    return { attention: items, channels: this.channelViews().filter((channel) => channel.unread > 0) };
  }

  private markAllRead(): void {
    for (const channel of this.channels) {
      const inChannel = this.messages.filter((message) => message.channel === channel.id).sort(compareMessages);
      const latest = inChannel.at(-1);
      if (latest) this.readMarkers.set(channel.id, { lamport: latest.lamport, author: latest.author });
      for (const root of inChannel) {
        const last = repliesOf(this.messages, root.id).sort(compareMessages).at(-1);
        if (last) this.readMarkers.set(root.id, { lamport: last.lamport, author: last.author });
      }
      this.context.notify({ kind: "messages", channel: channel.id });
    }
    this.context.notify({ kind: "channels" });
  }

  private mayDelete(id: string): boolean {
    return this.context.isAdmin() || id.startsWith(this.context.me().slice(0, 32));
  }

  private requireMayDelete(id: string): void {
    if (!this.mayDelete(id)) throw "only its owner or the admin can do that";
  }
}
