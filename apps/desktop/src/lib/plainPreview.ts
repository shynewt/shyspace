/**
 * One line of plain words for quotes, previews and search results, read with
 * the same markdown rules as the chat, so a link shows what it really opens.
 */

import { fromMarkdown } from "mdast-util-from-markdown";
import { ANY_MESSAGE_LINK, ANY_PAGE_LINK, MESSAGE_LINK, PAGE_LINK } from "./markdown";
import { isCheapToFormat } from "./markdownBudget";
import { everyoneMentioned, mentionedMemberId } from "./mentions";
import { displayName } from "./people";

/** Previews show a line or two; longer ones are cut after this many characters. */
const PREVIEW_CHARS = 300;

/** The parts of a markdown node a preview reads. */
type MarkdownNode = {
  type: string;
  value?: string;
  url?: string;
  alt?: string | null;
  /** A reference or definition's label, normalised by the parser so the two match. */
  identifier?: string;
  children?: MarkdownNode[];
};

type Names = {
  pageTitle: (id: string) => string | undefined;
  memberName: (id: string) => string | undefined;
  /** Where each `[label]: address` definition points, by identifier. */
  definitions: ReadonlyMap<string, string>;
};

/** Nodes that start a line of their own; their lines join with " · ". */
const BLOCKS = new Set(["paragraph", "heading", "code", "html", "tableCell"]);

/**
 * A whole message as one line: markdown marks dropped, lines joined with
 * " · ", and every app link named by what it opens, never by its author's
 * label: a member's current name, a page's title, "a linked message". The
 * message is read whole, so no link is cut. One the chat shows as plain text
 * (too costly to format) shows here as that plain text.
 */
export function plainPreview(
  text: string,
  pageTitle: (id: string) => string | undefined,
  memberName: (id: string) => string | undefined,
): string {
  if (!isCheapToFormat(text)) return shortened(oneLine(text.slice(0, PREVIEW_CHARS * 2)));
  const tree: MarkdownNode = fromMarkdown(text);
  const names = { pageTitle, memberName, definitions: definitionsIn(tree) };
  return shortened(oneLine(blockLines(tree, names).map(plainLine).filter(Boolean).join(" · ")));
}

function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function shortened(line: string): string {
  if (line.length <= PREVIEW_CHARS) return line;
  return `${line.slice(0, PREVIEW_CHARS).replace(/[\uD800-\uDBFF]$/, "")}…`;
}

function definitionsIn(tree: MarkdownNode): Map<string, string> {
  const definitions = new Map<string, string>();
  const visit = (node: MarkdownNode) => {
    // The first definition of a label wins, as in the chat's renderer.
    if (node.type === "definition" && node.identifier !== undefined && !definitions.has(node.identifier))
      definitions.set(node.identifier, node.url ?? "");
    for (const child of node.children ?? []) visit(child);
  };
  visit(tree);
  return definitions;
}

function blockLines(node: MarkdownNode, names: Names): string[] {
  if (node.type === "code" || node.type === "html") return (node.value ?? "").split("\n");
  if (BLOCKS.has(node.type)) return [inlineText(node, names)];
  return (node.children ?? []).flatMap((child) => blockLines(child, names));
}

function inlineText(node: MarkdownNode, names: Names): string {
  switch (node.type) {
    case "text":
      return withBareLinksNamed(node.value ?? "", names);
    case "inlineCode":
    case "html":
      return node.value ?? "";
    case "break":
      return " ";
    case "image":
    case "imageReference":
      return node.alt ?? "";
    case "link":
      return linkText(node.url ?? "", node, names);
    case "linkReference": {
      const url = node.identifier === undefined ? undefined : names.definitions.get(node.identifier);
      return url === undefined ? labelOf(node, names) : linkText(url, node, names);
    }
    default:
      return (node.children ?? []).map((child) => inlineText(child, names)).join("");
  }
}

function labelOf(link: MarkdownNode, names: Names): string {
  return (link.children ?? []).map((child) => inlineText(child, names)).join("");
}

function linkText(url: string, link: MarkdownNode, names: Names): string {
  const label = labelOf(link, names);
  const memberId = mentionedMemberId(url);
  if (memberId) return `@${names.memberName(memberId) ?? displayName("", memberId)}`;
  const everyone = everyoneMentioned(url);
  if (everyone) return `@${everyone}`;
  const page = url.match(PAGE_LINK)?.[1];
  if (page) return names.pageTitle(page) ?? "Untitled page";
  if (MESSAGE_LINK.test(url)) return "a linked message";
  return label;
}

function withBareLinksNamed(text: string, names: Names): string {
  return text
    .replace(ANY_PAGE_LINK, (_, id: string) => names.pageTitle(id) ?? "Untitled page")
    .replace(ANY_MESSAGE_LINK, "a linked message");
}

/** A callout's `[!note]`, a task's `[ ]` and strikethrough's `~~` read as text to the plain parser. */
function plainLine(line: string): string {
  return line
    .replace(/^\s*\[!(?:note|info|tip|important|warning|caution)\]\s*/i, "")
    .replace(/^\s*\[[ xX]\]\s+/, "")
    .replaceAll("~~", "")
    .trim();
}
