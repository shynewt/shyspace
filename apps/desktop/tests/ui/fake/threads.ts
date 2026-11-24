/** Thread rules, mirroring `crates/shy-core/src/storage/threads.rs`. */
import { mentions } from "../../../src/lib/mentions";

type Stored = { id: string; channel: string; reply_to: string | null; author: string; text: string };

function parentOf<T extends Stored>(messages: readonly T[], message: T): T | undefined {
  return messages.find((candidate) => candidate.id === message.reply_to && candidate.channel === message.channel);
}

/** A reply shows in the channel only when what it answered is missing. */
export function isInChannel<T extends Stored>(messages: readonly T[], message: T): boolean {
  return !message.reply_to || !parentOf(messages, message);
}

export function rootOf<T extends Stored>(messages: readonly T[], id: string): string | null {
  let current = messages.find((message) => message.id === id);
  for (let depth = 0; current && depth < 1000; depth += 1) {
    const parent = parentOf(messages, current);
    if (!parent) return current.id;
    current = parent;
  }
  return null;
}

export function repliesOf<T extends Stored>(messages: readonly T[], root: string): T[] {
  return messages.filter((message) => message.id !== root && rootOf(messages, message.id) === root);
}

export function followsByDefault(me: string, workspaceId: string, root: Stored, replies: readonly Stored[]): boolean {
  return [root, ...replies].some((message) => message.author === me || mentions(message.text, me, workspaceId));
}
