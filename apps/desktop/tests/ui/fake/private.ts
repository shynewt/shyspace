// Private channels and DMs, as the core shows them: who is in each, who owns
// it, and what you may do there. Keys and sealing are the core's business.

import type { Channel, PrivateMembers } from "../../../src/lib/api";
import type { ChatContext } from "./chat";
import { argument, ownedId, randomHex, requireString } from "./helpers";

/** Returned by `answer` for commands that aren't about private channels. */
export const NOT_PRIVATE = Symbol("not a private channel command");

type Kind = "private" | "dm" | "notes";
/** `joined` are members whose own op showed they read it; `isMissingKey`, that you missed a newer key. */
type StoredPrivate = { kind: Kind; members: string[]; owner: string | null; joined: string[]; isMissingKey: boolean };
export type PrivateView = Pick<Channel, "kind" | "dm_members" | "is_member" | "is_missing_key" | "can_delete">;

export type PrivateContext = ChatContext & {
  /** Lists a channel in chat under `name`. */
  listChannel: (id: string, name: string) => void;
};

function requireStringList(args: Parameters<typeof argument>[0], name: string): string[] {
  const value = argument(args, name);
  if (!Array.isArray(value) || !value.every((item) => typeof item === "string")) {
    throw new Error(`fake backend: "${name}" must be a list of strings`);
  }
  return value;
}

export class FakePrivate {
  private readonly channels = new Map<string, StoredPrivate>();
  private readonly context: PrivateContext;

  constructor(context: PrivateContext) {
    this.context = context;
  }

  answer(command: string, args: Parameters<typeof argument>[0]): unknown {
    switch (command) {
      case "create_private_channel":
        return this.create(requireString(args, "name"), requireStringList(args, "members"));
      case "open_dm":
        return this.openDm(requireStringList(args, "members"));
      case "open_notes":
        return this.openNotes();
      case "private_members":
        return this.membersOf(requireString(args, "channel"));
      case "add_to_private_channel":
        return this.add(requireString(args, "channel"), requireString(args, "member"));
      case "remove_from_private_channel":
        return this.remove(requireString(args, "channel"), requireString(args, "member"));
      case "hand_over_private_channel":
        return this.handOver(requireString(args, "channel"), requireString(args, "member"));
      case "leave_private_channel":
        return this.leave(requireString(args, "channel"));
      case "rotate_private_channel_key":
        this.requireOwner(requireString(args, "channel"));
        return undefined;
      default:
        return NOT_PRIVATE;
    }
  }

  /** How a channel shows, or null for a public one. */
  view(channel: string): PrivateView | null {
    const found = this.channels.get(channel);
    if (!found) return null;
    const me = this.context.me();
    return {
      kind: found.kind,
      dm_members: found.kind === "dm" ? found.members.filter((member) => member !== me) : [],
      is_member: found.members.includes(me),
      is_missing_key: found.isMissingKey && found.members.includes(me),
      can_delete: found.owner === me,
    };
  }

  /** Whether you may post in `channel`: anywhere public, or where you belong. */
  mayPost(channel: string): boolean {
    return this.view(channel)?.is_member ?? true;
  }

  /** Someone else made a private channel with you (and `others`) in it, as their ops would. */
  receive(owner: string, name: string, others: readonly string[]): string {
    const id = `p:${ownedId(owner)}${randomHex(8)}`;
    // Showing it to you opens it, so your device has joined too.
    const members = [owner, this.context.me(), ...others];
    this.channels.set(id, { kind: "private", members, owner, joined: [owner, this.context.me()], isMissingKey: false });
    this.context.listChannel(id, name);
    return id;
  }

  /** Someone else's change to a channel you're in, as their ops would bring it. */
  receiveChange(
    channel: string,
    change: { removed?: string; owner?: string; joined?: string; isMissingKey?: boolean },
  ): void {
    const found = this.require(channel);
    if (change.removed) found.members = found.members.filter((member) => member !== change.removed);
    if (change.owner) found.owner = change.owner;
    if (change.joined) found.joined.push(change.joined);
    if (change.isMissingKey !== undefined) found.isMissingKey = change.isMissingKey;
    this.changed();
  }

  private create(name: string, members: string[]): string {
    const me = this.context.me();
    const id = `p:${ownedId(me)}${randomHex(8)}`;
    this.channels.set(id, {
      kind: "private",
      members: [me, ...this.others(members)],
      owner: me,
      joined: [me],
      isMissingKey: false,
    });
    this.context.listChannel(id, name.trim().toLowerCase().replace(/\s+/g, "-"));
    return id;
  }

  private openDm(members: string[]): string {
    const everyone = [...new Set([this.context.me(), ...this.others(members)])].sort();
    if (everyone.length < 2 || everyone.length > 9) throw new Error("a DM is you and 1 to 8 others");
    for (const [id, found] of this.channels) {
      if (found.kind === "dm" && found.members.join() === everyone.join()) return id;
    }
    const id = `dm:${randomHex(16)}`;
    this.channels.set(id, { kind: "dm", members: everyone, owner: null, joined: everyone, isMissingKey: false });
    this.context.listChannel(id, "");
    return id;
  }

  private openNotes(): string {
    const me = this.context.me();
    for (const [id, found] of this.channels) {
      if (found.kind === "notes") return id;
    }
    const id = `dm:${randomHex(16)}`;
    this.channels.set(id, { kind: "notes", members: [me], owner: null, joined: [me], isMissingKey: false });
    this.context.listChannel(id, "");
    return id;
  }

  private membersOf(channel: string): PrivateMembers {
    const found = this.require(channel);
    return {
      members: [...found.members],
      invited: found.members.filter((member) => !found.joined.includes(member)),
      owner: found.owner,
      is_owner: found.owner === this.context.me(),
    };
  }

  private add(channel: string, member: string): void {
    const found = this.requireOwner(channel);
    const [added] = this.others([member]);
    if (!added) throw new Error("pick someone other than yourself");
    if (found.members.includes(added)) throw new Error("they are already in this channel");
    found.members.push(added);
    this.changed();
  }

  private remove(channel: string, member: string): void {
    const found = this.requireOwner(channel);
    if (member === this.context.me()) throw new Error("to leave a channel you own, hand it over first");
    if (!found.members.includes(member)) throw new Error("they aren't in this channel");
    found.members = found.members.filter((candidate) => candidate !== member);
    this.changed();
  }

  private handOver(channel: string, member: string): void {
    const found = this.requireOwner(channel);
    if (!found.members.includes(member) || member === this.context.me()) {
      throw new Error("hand it over to someone else in the channel");
    }
    found.owner = member;
    this.changed();
  }

  private leave(channel: string): void {
    const found = this.require(channel);
    const me = this.context.me();
    if (found.kind === "dm") throw new Error("you can't leave a DM");
    if (found.owner === me && found.members.length > 1) throw new Error("hand the channel over before leaving it");
    found.members = found.members.filter((member) => member !== me);
    this.changed();
  }

  /** Workspace members other than you, refusing anyone unknown. */
  private others(members: string[]): string[] {
    const me = this.context.me();
    const known = new Set(this.context.members().map((member) => member.id));
    for (const member of members) {
      if (!known.has(member)) throw new Error(`${member} isn't a member of this workspace`);
    }
    return [...new Set(members)].filter((member) => member !== me);
  }

  private require(channel: string): StoredPrivate {
    const found = this.channels.get(channel);
    if (!found) throw new Error(`fake backend: no private channel ${channel}`);
    return found;
  }

  private requireOwner(channel: string): StoredPrivate {
    const found = this.require(channel);
    if (found.kind !== "private") throw new Error("only private channels have owners");
    if (found.owner !== this.context.me()) throw new Error("only the channel's owner can do that");
    return found;
  }

  private changed(): void {
    this.context.tick();
    this.context.notify({ kind: "channels" });
  }
}
