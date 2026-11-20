/** Finding the links in a page: web and mail links, and links to other pages. */
import { syntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
import { isMentionLink, mentionedMemberId } from "../../../lib/mentions";

type SyntaxNode = ReturnType<typeof syntaxTree>["topNode"];

export const BARE_PAGE_LINK = /shy:\/\/page\/([0-9a-f]+)/g;
const PAGE_LINK = /^shy:\/\/page\/([0-9a-f]+)$/;
const SAFE_EXTERNAL = /^(https?:|mailto:)/i;
const EMAIL = /^[\w.+-]+@[\w-]+(\.[\w.-]+)+$/;

export type LinkTarget = { from: number; to: number; url: string; pageId: string | null };

export function pageIdOf(url: string): string | null {
  return url.match(PAGE_LINK)?.[1] ?? null;
}

/** The address a link opens in the browser or mail app, or null when it isn't one we open. */
export function externalUrl(url: string): string | null {
  if (SAFE_EXTERNAL.test(url)) return url;
  if (/^www\./i.test(url)) return `https://${url}`;
  if (EMAIL.test(url)) return `mailto:${url}`;
  return null;
}

/** The member mentioned by the mention chip at `position` (either side of it), if any. */
export function mentionAt(state: EditorState, position: number): string | null {
  const tree = syntaxTree(state);
  for (const side of [1, -1] as const) {
    for (let node: SyntaxNode | null = tree.resolveInner(position, side); node; node = node.parent) {
      if (node.name !== "Link") continue;
      const url = node.getChild("URL");
      return url ? mentionedMemberId(state.sliceDoc(url.from, url.to)) : null;
    }
  }
  return null;
}

/** The link at `position`, if any. Mentions aren't links: their chips open a profile card instead. */
export function linkAt(state: EditorState, position: number): LinkTarget | null {
  const tree = syntaxTree(state);
  for (const side of [1, -1] as const) {
    for (let node: SyntaxNode | null = tree.resolveInner(position, side); node; node = node.parent) {
      if (node.name === "Link") return linkOf(state, node, node.getChild("URL"));
      if (node.name === "URL" && node.parent?.name !== "Image") return linkOf(state, node, node);
    }
  }
  const line = state.doc.lineAt(position);
  for (const match of line.text.matchAll(BARE_PAGE_LINK)) {
    const from = line.from + match.index;
    const to = from + match[0].length;
    if (from <= position && position <= to) return { from, to, url: match[0], pageId: match[1] };
  }
  return null;
}

function linkOf(state: EditorState, node: SyntaxNode, url: SyntaxNode | null): LinkTarget | null {
  if (!url) return null;
  const href = state.sliceDoc(url.from, url.to);
  if (isMentionLink(href)) return null;
  return { from: node.from, to: node.to, url: href, pageId: pageIdOf(href) };
}
