/**
 * Mentions are markdown links in a message: `[@Ada](shy://<workspace>/member/<id>)`,
 * `[@channel](shy://<workspace>/mention/channel)`, `[@here](…/mention/here)`;
 * older ones name no workspace (see `appLinks.ts`). Only mentions in the
 * workspace they're read in count. These helpers mirror
 * `crates/shy-core/src/mentions.rs`.
 */

import { appLink, appLinkPattern, isInWorkspace, parseAppLink } from "./appLinks";

const EVERYONE = ["channel", "here"] as const;
const ANY_MEMBER_LINK = appLinkPattern("member");
const ANY_EVERYONE_LINK = appLinkPattern("mention");

export function memberLink(workspaceId: string, name: string, id: string): string {
  return `[@${name.replace(/[[\]]/g, "")}](${appLink(workspaceId, "member", id)})`;
}

function mentionHere(href: string, workspaceId: string) {
  const link = parseAppLink(href);
  if (!link || !isInWorkspace(link, workspaceId)) return null;
  return link.kind === "member" || link.kind === "mention" ? link : null;
}

/** Whether `href` mentions someone in `workspaceId`; a mention naming another workspace isn't one here. */
export function isMentionLink(href: string, workspaceId: string): boolean {
  return mentionHere(href, workspaceId) !== null;
}

/** The member a mention link in `workspaceId` names; null for @channel, @here and other links. */
export function mentionedMemberId(href: string, workspaceId: string): string | null {
  const link = mentionHere(href, workspaceId);
  return link?.kind === "member" ? link.id : null;
}

/** "channel" or "here" for an @channel or @here link in `workspaceId`; null for any other. */
export function everyoneMentioned(href: string, workspaceId: string): "channel" | "here" | null {
  const link = mentionHere(href, workspaceId);
  return link?.kind === "mention" && (link.id === "channel" || link.id === "here") ? link.id : null;
}

/** Whether a mention link reaches `memberId` of `workspaceId`: theirs, or @channel / @here. */
export function linkMentions(href: string, memberId: string, workspaceId: string): boolean {
  return everyoneMentioned(href, workspaceId) !== null || mentionedMemberId(href, workspaceId) === memberId;
}

/** Whether `text` mentions `memberId` of `workspaceId`, like the core decides. */
export function mentions(text: string, memberId: string, workspaceId: string): boolean {
  const here = (match: RegExpMatchArray) => isInWorkspace({ workspace: match[1] ?? null }, workspaceId);
  if ([...text.matchAll(ANY_EVERYONE_LINK)].some(here)) return true;
  return [...text.matchAll(ANY_MEMBER_LINK)].some((match) => here(match) && match[2] === memberId);
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
  workspaceId: string,
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
    if (name === "channel" || name === "here") return `[@${name}](${appLink(workspaceId, "mention", name)})`;
    const id = picked.get(name);
    if (id) return memberLink(workspaceId, name, id);
    const page = pickedPages.get(name);
    return page ? appLink(workspaceId, "page", page) : whole;
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
