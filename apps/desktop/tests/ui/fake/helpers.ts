import type { InvokeArgs } from "@tauri-apps/api/core";
import type { NotifyLevel } from "../../../src/lib/api";

export type Kind = { kind: "person" | "bot" };

export function randomHex(bytes: number): string {
  const values = crypto.getRandomValues(new Uint8Array(bytes));
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
