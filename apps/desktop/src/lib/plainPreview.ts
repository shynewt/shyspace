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
const CUT = "…";

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

/**
 * The same for text already cut, marked by "…" at a cut end (a search
 * snippet, a thread's latest reply): a link split by a cut has lost what
 * names it, so none of its label shows.
 */
export function cutPreview(
  text: string,
  pageTitle: (id: string) => string | undefined,
  memberName: (id: string) => string | undefined,
): string {
  let source = text;
  if (source.startsWith(CUT)) source = withoutCutStart(source);
  if (source.endsWith(CUT)) source = withoutCutEnd(source);
  const preview = plainPreview(source, pageTitle, memberName);
  return source.endsWith(CUT) && !preview.endsWith(CUT) ? `${preview}${CUT}` : preview;
}

function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function shortened(line: string): string {
  if (line.length <= PREVIEW_CHARS) return line;
  return `${line.slice(0, PREVIEW_CHARS).replace(/[\uD800-\uDBFF]$/, "")}${CUT}`;
}

/** Where `]` at `index` ends its link: past an address `(…)` or a reference `[…]` after it, if any. */
function linkEnd(source: string, index: number): number {
  const next = source[index + 1];
  const closer = next === "(" ? ")" : next === "[" ? "]" : null;
  if (!closer) return index + 1;
  const close = source.indexOf(closer, index + 2);
  return close === -1 ? source.length : close + 1;
}

/**
 * The last index of the backtick run at `index`, or of the code span it opens:
 * as in markdown, a span closes at the next run of the same length, and a run
 * with none is plain text, so brackets after it still count.
 */
function codeSpanEnd(source: string, index: number): number {
  let runEnd = index;
  while (source[runEnd + 1] === "`") runEnd++;
  const fence = source.slice(index, runEnd + 1);
  for (let close = source.indexOf(fence, runEnd + 1); close !== -1; close = source.indexOf(fence, close + 1)) {
    const isExactRun = source[close - 1] !== "`" && source[close + fence.length] !== "`";
    if (isExactRun) return close + fence.length - 1;
  }
  return runEnd;
}

/** Whether `rest` (what follows a `]` up to the cut) is an address or reference the cut left open. */
function isOpenTarget(rest: string): boolean {
  if (rest === "") return true;
  if (rest.startsWith("[")) return !rest.includes("]");
  if (!rest.startsWith("(")) return false;
  let depth = 0;
  for (const character of rest) {
    if (character === "(") depth++;
    else if (character === ")" && --depth === 0) return false;
  }
  return true;
}

/** A snippet that starts inside a link: up to the end of the link whose `[` the cut took. */
function withoutCutStart(source: string): string {
  let depth = 0;
  for (let index = CUT.length; index < source.length; index++) {
    const character = source[index];
    if (character === "\\") index++;
    else if (character === "`") index = codeSpanEnd(source, index);
    else if (character === "[") depth++;
    else if (character === "]" && depth > 0) depth--;
    else if (character === "]") return `${CUT}${source.slice(linkEnd(source, index))}`;
  }
  return source;
}

/**
 * A snippet that ends inside a link: from the outermost `[` still open at
 * the cut, or a bracket closed only for its address or reference to be cut.
 */
function withoutCutEnd(source: string): string {
  const body = source.slice(0, -CUT.length);
  const open: number[] = [];
  let lastClosed: { open: number; close: number } | null = null;
  for (let index = 0; index < body.length; index++) {
    const character = body[index];
    if (character === "\\") index++;
    else if (character === "`") index = codeSpanEnd(body, index);
    else if (character === "[") open.push(index);
    else if (character === "]") {
      const opened = open.pop();
      if (opened !== undefined) lastClosed = { open: opened, close: index };
    }
  }
  let cut = open[0] ?? body.length;
  if (lastClosed && lastClosed.open < cut && isOpenTarget(body.slice(lastClosed.close + 1, cut))) {
    cut = lastClosed.open;
  }
  return `${body.slice(0, cut).trimEnd()}${CUT}`;
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
  if (memberId) return truthfulLabel(label, `@${names.memberName(memberId) ?? displayName("", memberId)}`);
  const everyone = everyoneMentioned(url);
  if (everyone) return truthfulLabel(label, `@${everyone}`);
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

/**
 * `label` when it already says `truth`, ignoring control characters (a search
 * snippet's match marks, kept so the match stays highlighted); else `truth`.
 */
function truthfulLabel(label: string, truth: string): string {
  return label.replace(/\p{Cc}/gu, "") === truth ? label : truth;
}

/** A callout's `[!note]`, a task's `[ ]` and strikethrough's `~~` read as text to the plain parser. */
function plainLine(line: string): string {
  return line
    .replace(/^\s*\[!(?:note|info|tip|important|warning|caution)\]\s*/i, "")
    .replace(/^\s*\[[ xX]\]\s+/, "")
    .replaceAll("~~", "")
    .trim();
}
