/** Finding the links in a page: web and mail links, and links to other pages and to messages. */
import { syntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
import { type AppLink, appLinkPattern, isInWorkspace, parseAppLink } from "../../../lib/appLinks";
import { isMentionLink, mentionedMemberId } from "../../../lib/mentions";
import { workspaceLinksOf } from "./openLinks";

type SyntaxNode = ReturnType<typeof syntaxTree>["topNode"];

/** A page or message link anywhere in text: its workspace (if it names one) in group 1, its id in group 2. */
export const BARE_PAGE_LINK = appLinkPattern("page");
export const BARE_MESSAGE_LINK = appLinkPattern("message");
const SAFE_EXTERNAL = /^(https?:|mailto:)/i;
const EMAIL = /^[\w.+-]+@[\w-]+(\.[\w.-]+)+$/;

/** `text` is what the link shows: its label, or the address itself. */
export type LinkTarget = {
  from: number;
  to: number;
  url: string;
  text: string;
  pageId: string | null;
  messageId: string | null;
  /** A link into another workspace, which opens there. */
  elsewhere: AppLink | null;
};

/** The page `url` links to in `workspaceId`. */
export function pageIdOf(url: string, workspaceId: string): string | null {
  return idHere(url, "page", workspaceId);
}

/** The message `url` links to in `workspaceId`. */
export function messageIdOf(url: string, workspaceId: string): string | null {
  return idHere(url, "message", workspaceId);
}

function idHere(url: string, kind: "page" | "message", workspaceId: string): string | null {
  const link = parseAppLink(url);
  return link?.kind === kind && isInWorkspace(link, workspaceId) ? link.id : null;
}

/** `url` as a link into another workspace than `workspaceId`, if it is one. */
export function elsewhereOf(url: string, workspaceId: string): AppLink | null {
  const link = parseAppLink(url);
  return link && !isInWorkspace(link, workspaceId) ? link : null;
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
      return url ? mentionedMemberId(state.sliceDoc(url.from, url.to), workspaceLinksOf(state).workspaceId) : null;
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
  const { workspaceId } = workspaceLinksOf(state);
  const line = state.doc.lineAt(position);
  for (const pattern of [BARE_PAGE_LINK, BARE_MESSAGE_LINK]) {
    for (const match of line.text.matchAll(pattern)) {
      const from = line.from + match.index;
      const to = from + match[0].length;
      if (from <= position && position <= to) {
        return {
          from,
          to,
          url: match[0],
          text: match[0],
          pageId: pageIdOf(match[0], workspaceId),
          messageId: messageIdOf(match[0], workspaceId),
          elsewhere: elsewhereOf(match[0], workspaceId),
        };
      }
    }
  }
  return null;
}

function linkOf(state: EditorState, node: SyntaxNode, url: SyntaxNode | null): LinkTarget | null {
  if (!url) return null;
  const href = state.sliceDoc(url.from, url.to);
  const { workspaceId } = workspaceLinksOf(state);
  if (isMentionLink(href, workspaceId)) return null;
  const elsewhere = elsewhereOf(href, workspaceId);
  // A mention of someone in another workspace is its text: nothing to open.
  if (elsewhere?.kind === "member" || elsewhere?.kind === "mention") return null;
  const marks = node.getChildren("LinkMark");
  const text = marks.length >= 2 ? state.sliceDoc(marks[0].to, marks[1].from) : href;
  return {
    from: node.from,
    to: node.to,
    url: href,
    text,
    pageId: pageIdOf(href, workspaceId),
    messageId: messageIdOf(href, workspaceId),
    elsewhere,
  };
}
