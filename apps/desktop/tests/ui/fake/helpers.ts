import type { InvokeArgs } from "@tauri-apps/api/core";
import type { NotifyLevel } from "../../../src/lib/api";

export type Kind = { kind: "person" | "bot" };

let nextSeededByte: (() => number) | null = null;

/** From now on ids repeat for the same seed, so a test can reload the app and find the same things. */
export function seedIds(seed: string): void {
  let state = 0;
  for (const character of seed) state = (Math.imul(state, 31) + character.charCodeAt(0)) | 0;
  // mulberry32: small and good enough to make ids that don't collide in a test.
  nextSeededByte = () => {
    state = (state + 0x6d2b79f5) | 0;
    let mixed = Math.imul(state ^ (state >>> 15), 1 | state);
    mixed = (mixed + Math.imul(mixed ^ (mixed >>> 7), 61 | mixed)) ^ mixed;
    return ((mixed ^ (mixed >>> 14)) >>> 0) & 0xff;
  };
}

export function randomHex(bytes: number): string {
  const values = nextSeededByte
    ? Uint8Array.from({ length: bytes }, nextSeededByte)
    : crypto.getRandomValues(new Uint8Array(bytes));
  return Array.from(values, (value) => value.toString(16).padStart(2, "0")).join("");
}

/** Like the core: an id starts with 16 bytes of its owner's key. */
export function ownedId(owner: string): string {
  return `${owner.slice(0, 32)}${randomHex(8)}`;
}

export function argument(args: InvokeArgs | undefined, name: string): unknown {
  if (!args || Array.isArray(args) || args instanceof ArrayBuffer || args instanceof Uint8Array) return undefined;
  return args[name];
}

export function requireString(args: InvokeArgs | undefined, name: string): string {
  const value = argument(args, name);
  if (typeof value !== "string") throw new Error(`fake backend: missing string argument "${name}"`);
  return value;
}

const NOTIFY_LEVELS: readonly NotifyLevel[] = ["all", "mentions", "nothing"];

export function requireNotifyLevel(args: InvokeArgs | undefined, name: string): NotifyLevel {
  const value = requireString(args, name);
  const level = NOTIFY_LEVELS.find((candidate) => candidate === value);
  if (!level) throw new Error(`fake backend: "${value}" is not a notification level`);
  return level;
}
