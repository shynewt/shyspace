/**
 * Shared files and pictures in the fake: staged tokens, stored bytes, and
 * the events the native side sends for dropped files. No encryption or
 * bucket; those are covered by the Rust tests.
 */

import type { InvokeArgs } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import type { Change, FileInfo, Member, StagedFile } from "../../../src/lib/api";
import { argument, ownedId, randomHex, requireString } from "./helpers";

/** Returned by `answer` for commands that aren't about files. */
export const NOT_FILES = Symbol("not a files command");

export type FileForTest = { name: string; type: string; bytes: number[] };

export type FilesContext = {
  me: () => string;
  isAdmin: () => boolean;
  members: () => Member[];
  notify: (change: Change) => void;
  /** Posts a message as this member, like `send_message`, and returns its id. */
  sendMessage: (channel: string, text: string, replyTo: string | null) => string;
  channelOf: (message: string) => string | null;
  isPage: (id: string) => boolean;
};

type StoredFile = FileInfo & { bytes: number[] };

/** A 1×1 PNG, the smallest real image the app will show. */
export const TINY_PNG = [
  137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137,
  0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73,
  69, 78, 68, 174, 66, 96, 130,
];

export class FakeFiles {
  private readonly context: FilesContext;
  private staged = new Map<string, FileForTest>();
  private files: StoredFile[] = [];
  private pictures = new Map<string, number[]>();
  private nextToken = 0;
  /** What the next `pick_files` returns, as if the person picked them. */
  pickNext: FileForTest[] = [];
  /** File ids the person saved, in order. */
  readonly saved: string[] = [];
  /** Makes the next send fail, like a bucket that can't be reached. */
  failNextSend = false;

  constructor(context: FilesContext) {
    this.context = context;
  }

  answer(command: string, args: InvokeArgs | undefined): unknown {
    switch (command) {
      case "pick_files": {
        const picked = this.pickNext.map((file) => this.stage(file));
        this.pickNext = [];
        return picked;
      }
      case "stage_pasted_file":
        return this.stage(unframePastedFile(args));
      case "staged_file_bytes": {
        const file = this.staged.get(requireString(args, "token"));
        if (!file) throw new Error("that file is no longer waiting to be sent");
        return new Uint8Array(file.bytes).buffer;
      }
      case "unstage_file":
        this.staged.delete(requireString(args, "token"));
        return null;
      case "send_files":
        return this.send(
          requireString(args, "channel"),
          requireString(args, "text"),
          (argument(args, "replyTo") as string | null) ?? null,
          argument(args, "tokens") as string[],
        );
      case "file_bytes":
        return new Uint8Array(this.find(requireString(args, "file")).bytes).buffer;
      case "share_page_file":
        return this.shareInPage(requireString(args, "page"), requireString(args, "token"));
      case "page_files":
        return this.filesOfPage(requireString(args, "page")).map((file) => this.present(file));
      case "page_file_bytes":
        return new Uint8Array(this.findInPage(requireString(args, "page"), requireString(args, "file")).bytes).buffer;
      case "save_page_file":
        this.saved.push(this.findInPage(requireString(args, "page"), requireString(args, "file")).id);
        return true;
      case "save_file":
        this.saved.push(this.find(requireString(args, "file")).id);
        return true;
      case "delete_file":
        return this.delete(requireString(args, "file"));
      case "rename_file":
        return this.rename(requireString(args, "file"), requireString(args, "name"));
      case "files":
        return this.files.map((file) => this.present(file)).reverse();
      case "profile_picture":
        return new Uint8Array(this.pictures.get(requireString(args, "member")) ?? []).buffer;
      case "set_profile_picture":
        return this.setPicture(this.context.me(), argument(args, "image") as number[]);
      default:
        return NOT_FILES;
    }
  }

  /** The files shared with a message, as the app sees them. */
  filesOf(message: string): FileInfo[] {
    return this.files.filter((file) => file.message === message).map((file) => this.present(file));
  }

  /** Forgets the files of a deleted page, as the core does. */
  dropFilesOfPage(page: string): void {
    this.files = this.files.filter((file) => file.place !== page);
  }

  /** Forgets the files of a deleted message. */
  dropFilesOf(message: string): void {
    this.files = this.files.filter((file) => file.message !== message);
  }

  // ---- Acting for tests ---------------------------------------------------

  /** Files dropped on the window: staged, then announced like the native side does. */
  drop(files: FileForTest[]): void {
    const staged = files.map((file) => this.stage(file));
    void emit("file-drag", false);
    void emit("files-dropped", staged);
  }

  dragOver(isDragging: boolean): void {
    void emit("file-drag", isDragging);
  }

  /** A file someone else shared into a page. */
  receiveInPage(author: string, page: string, file: FileForTest): string {
    if (!this.context.isPage(page)) throw new Error("fake files: no such page");
    const id = ownedId(author);
    this.files.push(this.stored(id, author, page, null, file));
    this.context.notify({ kind: "page_files", page });
    return id;
  }

  /** A file someone else shared with a new message. */
  receive(author: string, message: string, file: FileForTest): string {
    const channel = this.context.channelOf(message);
    if (!channel) throw new Error("fake files: no such message");
    const id = ownedId(author);
    this.files.push(this.stored(id, author, channel, message, file));
    this.context.notify({ kind: "messages", channel });
    return id;
  }

  setPicture(member: string, image: number[]): null {
    const target = this.context.members().find((candidate) => candidate.id === member);
    if (!target) throw new Error("fake files: no such member");
    if (image.length === 0) {
      this.pictures.delete(member);
      target.picture = null;
    } else {
      this.pictures.set(member, image);
      target.picture = randomHex(16);
    }
    this.context.notify({ kind: "members" });
    return null;
  }

  // ---- Inside -------------------------------------------------------------

  private stage(file: FileForTest): StagedFile {
    const token = String(this.nextToken++);
    this.staged.set(token, file);
    return { token, name: file.name, size: file.bytes.length, media_type: file.type };
  }

  private send(channel: string, text: string, replyTo: string | null, tokens: string[]): string {
    if (this.failNextSend) {
      this.failNextSend = false;
      throw new Error("couldn't reach the bucket");
    }
    const files = tokens.map((token) => {
      const file = this.staged.get(token);
      if (!file) throw new Error("that file is no longer waiting to be sent");
      return file;
    });
    const total = files.reduce((sum, file) => sum + file.bytes.length, 0);
    this.context.notify({ kind: "upload", channel, sent: total, total });
    const message = this.context.sendMessage(channel, text, replyTo);
    for (const file of files)
      this.files.push(this.stored(ownedId(this.context.me()), this.context.me(), channel, message, file));
    for (const token of tokens) this.staged.delete(token);
    this.context.notify({ kind: "messages", channel });
    return message;
  }

  private shareInPage(page: string, token: string): string {
    const file = this.staged.get(token);
    if (!file) throw new Error("that file is no longer waiting to be sent");
    if (!this.context.isPage(page)) throw new Error("no such page here");
    this.context.notify({ kind: "upload", channel: page, sent: file.bytes.length, total: file.bytes.length });
    const id = ownedId(this.context.me());
    this.files.push(this.stored(id, this.context.me(), page, null, file));
    this.staged.delete(token);
    this.context.notify({ kind: "page_files", page });
    return id;
  }

  private filesOfPage(page: string): StoredFile[] {
    return this.files.filter((file) => file.place === page && file.message === null);
  }

  /** Like the core: a page reads only its own files. */
  private findInPage(page: string, id: string): StoredFile {
    const file = this.find(id);
    if (file.place !== page || file.message !== null) throw new Error("that file isn't part of this page");
    return file;
  }

  private changeFor(file: StoredFile): Change {
    return file.message === null ? { kind: "page_files", page: file.place } : { kind: "messages", channel: file.place };
  }

  private stored(id: string, author: string, place: string, message: string | null, file: FileForTest): StoredFile {
    return {
      id,
      place,
      message,
      author,
      name: file.name,
      media_type: file.type || "application/octet-stream",
      size: file.bytes.length,
      created_ms: Date.now(),
      can_delete: false,
      can_rename: false,
      bytes: file.bytes,
    };
  }

  private present({ bytes: _bytes, ...file }: StoredFile): FileInfo {
    return {
      ...file,
      can_delete: file.author === this.context.me() || this.context.isAdmin(),
      can_rename: file.author === this.context.me(),
    };
  }

  private rename(id: string, name: string): null {
    const file = this.find(id);
    if (file.author !== this.context.me()) throw new Error("only the person who shared a file can rename it");
    if (!name.trim()) throw new Error("a file needs a name");
    file.name = name.trim();
    this.context.notify(this.changeFor(file));
    return null;
  }

  private find(id: string): StoredFile {
    const file = this.files.find((candidate) => candidate.id === id);
    if (!file) throw new Error("no such file here");
    return file;
  }

  private delete(id: string): null {
    const file = this.find(id);
    if (file.author !== this.context.me() && !this.context.isAdmin())
      throw new Error("only its owner or the admin can do that");
    this.files = this.files.filter((candidate) => candidate.id !== id);
    this.context.notify(this.changeFor(file));
    return null;
  }
}

/** Reads what `framePastedFile` in lib/api.ts sends, as the Rust side does. */
function unframePastedFile(args: InvokeArgs | undefined): FileForTest {
  if (!(args instanceof Uint8Array)) throw new Error("a pasted file must arrive as raw bytes");
  const decoder = new TextDecoder();
  let offset = 0;
  const texts: string[] = [];
  for (let index = 0; index < 2; index++) {
    const length = (args[offset] << 8) | args[offset + 1];
    texts.push(decoder.decode(args.subarray(offset + 2, offset + 2 + length)));
    offset += 2 + length;
  }
  return { name: texts[0], type: texts[1], bytes: Array.from(args.subarray(offset)) };
}
