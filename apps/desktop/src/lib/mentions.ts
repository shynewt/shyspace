/**
 * Mentions are markdown links in a message: `[@Ada](shy://member/<id>)`,
 * `[@channel](shy://mention/channel)`, `[@here](shy://mention/here)`. These
 * helpers mirror `crates/shy-core/src/mentions.rs`.
 */

import { PAGE_LINK_PREFIX } from "./api";

const MEMBER_LINK = /^shy:\/\/member\/([0-9a-f]+)$/;
const EVERYONE_LINK = /^shy:\/\/mention\/(channel|here)$/;
const EVERYONE = ["channel", "here"] as const;

export function memberLink(name: string, id: string): string {
  return `[@${name.replace(/[[\]]/g, "")}](shy://member/${id})`;
}

export function isMentionLink(href: string): boolean {
  return MEMBER_LINK.test(href) || EVERYONE_LINK.test(href);
}

/** The member a `shy://member/<id>` link mentions; null for @channel, @here and other links. */
export function mentionedMemberId(href: string): string | null {
  return href.match(MEMBER_LINK)?.[1] ?? null;
}

/** Whether a mention link reaches `memberId`: theirs, or @channel / @here. */
export function linkMentions(href: string, memberId: string): boolean {
  return EVERYONE_LINK.test(href) || href.match(MEMBER_LINK)?.[1] === memberId;
}

/** Whether `text` mentions `memberId`, like the core decides. */
export function mentions(text: string, memberId: string): boolean {
  if (/shy:\/\/mention\/(channel|here)/.test(text)) return true;
  return [...text.matchAll(/shy:\/\/member\/([0-9a-f]+)/g)].some((match) => match[1] === memberId);
}

/** The `@query` being typed right before the caret, if any. */
export function mentionQueryAt(text: string, caret: number): { start: number; query: string } | null {
  const match = text.slice(0, caret).match(/(?:^|\s)@([\p{L}\p{N}_.-]*)$/u);
  if (!match) return null;
  return { start: caret - match[1].length - 1, query: match[1] };
}

function escapeForRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Turns `@Name` for people picked from suggestions, plus `@channel` and
 * `@here`, into mention links, and `@Title` for picked pages into page links.
 * One pass, longest names first, so "@Ada Lovelace" never becomes a link to
 * "Ada" followed by "Lovelace".
 */
export function withMentionLinks(
  text: string,
  picked: ReadonlyMap<string, string>,
  pickedPages: ReadonlyMap<string, string> = new Map(),
): string {
  const names = [...new Set([...picked.keys(), ...pickedPages.keys(), ...EVERYONE])].sort(
    (left, right) => right.length - left.length,
  );
  const pattern = new RegExp(
    `(?<![\\[\\p{L}\\p{N}_])@(${names.map(escapeForRegex).join("|")})(?![\\p{L}\\p{N}_])`,
    "gu",
  );
  return text.replace(pattern, (whole, name: string) => {
    if (name === "channel" || name === "here") return `[@${name}](shy://mention/${name})`;
    const id = picked.get(name);
    if (id) return memberLink(name, id);
    const page = pickedPages.get(name);
    return page ? `${PAGE_LINK_PREFIX}${page}` : whole;
  });
}

/** The one person whose whole name is `typed`, ignoring case; null if none or several. */
export function exactlyNamed<Person extends { name: string }>(typed: string, people: readonly Person[]): Person | null {
  const wanted = typed.toLowerCase();
  const matching = people.filter((person) => person.name.toLowerCase() === wanted);
  return matching.length === 1 ? matching[0] : null;
}

/**
 * Writes each `@name` that is exactly one person's whole name (ignoring case)
 * as their name, and returns who each is, so it becomes a mention without
 * being picked. Anything else, like a prefix or a name two people share, stays
 * plain text.
 */
export function withTypedMentions(
  text: string,
  people: readonly { name: string; id: string }[],
): { text: string; named: Map<string, string> } {
  const named = new Map<string, string>();
  const replaced = text.replace(/(?<![[\p{L}\p{N}_])@([\p{L}\p{N}_-]+)(?![\p{L}\p{N}_])/gu, (whole, typed: string) => {
    const person = exactlyNamed(typed, people);
    if (!person) return whole;
    named.set(person.name, person.id);
    return `@${person.name}`;
  });
  return { text: replaced, named };
}
