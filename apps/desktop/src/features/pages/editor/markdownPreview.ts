/**
 * Decides how a page's markdown looks in the live preview: which marks to
 * hide, which to draw as widgets, and which lines to style. Pure: it reads
 * the syntax tree and returns ranges; `livePreview.ts` turns them into
 * CodeMirror decorations. The document itself is never touched.
 */
import type { syntaxTree } from "@codemirror/language";
import type { Text } from "@codemirror/state";
import type { AppLink } from "../../../lib/appLinks";
import { type CalloutKind, calloutMarker } from "../../../lib/callouts";
import { isMentionLink, mentionedMemberId } from "../../../lib/mentions";
import { fileIdOf, parseFileImageUrl, unescapeLinkText } from "./images";
import { BARE_MESSAGE_LINK, BARE_PAGE_LINK, elsewhereOf, messageIdOf, pageIdOf } from "./links";

export type SyntaxTree = ReturnType<typeof syntaxTree>;
type SyntaxNode = SyntaxTree["topNode"];

export type PreviewItem =
  /** Text the reader doesn't see: syntax marks on lines nobody is editing. */
  | { kind: "hide"; from: number; to: number }
  /** A class on the whole line starting at `from`. */
  | { kind: "line"; from: number; className: string }
  | { kind: "style"; from: number; to: number; className: string }
  | { kind: "bullet"; from: number; to: number; depth: number }
  | { kind: "checkbox"; from: number; to: number; isChecked: boolean; label: string }
  | { kind: "divider"; from: number; to: number }
  /** A callout's `[!KIND]` marker, drawn as its label. */
  | { kind: "callout"; from: number; to: number; callout: CalloutKind }
  | { kind: "page"; from: number; to: number; pageId: string }
  /** `label` is the link's own text, or "a message" for a bare link, as in chat. */
  | { kind: "message"; from: number; to: number; messageId: string; label: string }
  /** `memberId` is null for @channel and @here. */
  | { kind: "mention"; from: number; to: number; label: string; memberId: string | null }
  /** A link to one of the page's files, drawn as a chip; `label` is the link's text. */
  | { kind: "file"; from: number; to: number; fileId: string; label: string }
  /** An image of one of the page's files; `width` is a percent of the column, or null for its own size. */
  | { kind: "image"; from: number; to: number; fileId: string; width: number | null; caption: string }
  /** A page or message link into another workspace. */
  | { kind: "elsewhere"; from: number; to: number; link: AppLink };

const BARE_MESSAGE_LABEL = "a message";

/** Where a bare `shy://page/…` or `shy://message/…` link is plain text, not a link to draw as a chip. */
const NOT_A_CHIP = new Set(["Link", "Image", "Autolink", "URL", "InlineCode", "FencedCode", "CodeBlock", "HTMLBlock"]);

/**
 * The preview for the parts of `doc` in `ranges` (the visible ones, so
 * long pages stay fast), from its syntax `tree` as far as it's parsed. Marks
 * on `revealedLines` stay visible, so the line being edited reads as
 * markdown; page and message links and mentions are chips always.
 */
export function previewItems(
  doc: Text,
  tree: SyntaxTree,
  ranges: readonly { from: number; to: number }[],
  revealedLines: ReadonlySet<number>,
  workspaceId: string,
): PreviewItem[] {
  const builder = new PreviewBuilder(doc, revealedLines, workspaceId);
  for (const range of ranges) {
    tree.iterate({ from: range.from, to: range.to, enter: (node) => builder.visit(node.node, range) });
    builder.addBareAppLinks(tree.topNode, range);
  }
  return builder.items();
}

class PreviewBuilder {
  private readonly found = new Map<string, PreviewItem>();

  constructor(
    private readonly doc: Text,
    private readonly revealedLines: ReadonlySet<number>,
    private readonly workspaceId: string,
  ) {}

  items(): PreviewItem[] {
    return [...this.found.values()];
  }

  /** Returns false to skip the node's children. */
  visit(node: SyntaxNode, range: { from: number; to: number }): boolean | undefined {
    switch (node.name) {
      case "ATXHeading1":
      case "ATXHeading2":
      case "ATXHeading3":
      case "ATXHeading4":
      case "ATXHeading5":
      case "ATXHeading6":
        this.add({ kind: "line", from: this.doc.lineAt(node.from).from, className: `cm-md-h${node.name.slice(-1)}` });
        return;
      case "SetextHeading1":
      case "SetextHeading2":
        // The last line is the `===` or `---` underline, left as written.
        this.styleLines(node.from, this.doc.lineAt(node.to).from - 1, range, () => `cm-md-h${node.name.slice(-1)}`);
        return;
      case "HeaderMark":
        this.hideHeaderMark(node);
        return;
      case "EmphasisMark":
      case "StrikethroughMark":
        this.hide(node.from, node.to);
        return;
      case "Escape":
        this.hide(node.from, node.from + 1);
        return;
      case "Blockquote":
        this.styleQuote(node, range);
        return;
      case "QuoteMark":
        this.hide(node.from, this.afterOneSpace(node.to));
        return;
      case "InlineCode":
        this.add({ kind: "style", from: node.from, to: node.to, className: "cm-md-code" });
        for (const mark of node.getChildren("CodeMark")) this.hide(mark.from, mark.to);
        return false;
      case "FencedCode":
      case "CodeBlock":
        this.styleCodeBlock(node, range);
        return false;
      case "ListMark":
        this.addListMark(node);
        return;
      case "TaskMarker":
        if (isChecked(this.doc.sliceString(node.from, node.to)) && node.parent) {
          const textFrom = Math.min(this.afterSpaces(node.to), node.parent.to);
          this.add({ kind: "style", from: textFrom, to: node.parent.to, className: "cm-md-task-done" });
        }
        return;
      case "Link":
        return this.addLink(node);
      case "Autolink":
        for (const mark of node.getChildren("LinkMark")) this.hide(mark.from, mark.to);
        return;
      case "URL":
        if (node.parent?.name !== "Link" && node.parent?.name !== "Image") {
          this.add({ kind: "style", from: node.from, to: node.to, className: "cm-md-link" });
        }
        return;
      case "HorizontalRule":
        if (!this.isRevealed(node.from)) this.add({ kind: "divider", from: node.from, to: node.to });
        return;
      case "Image":
        this.addImage(node);
        return false;
      case "HTMLBlock":
      case "CommentBlock":
        return false;
    }
  }

  addBareAppLinks(top: SyntaxNode, range: { from: number; to: number }) {
    const text = this.doc.sliceString(range.from, range.to);
    for (const match of text.matchAll(BARE_PAGE_LINK)) {
      const from = range.from + match.index;
      if (isInside(top, from, NOT_A_CHIP)) continue;
      const to = from + match[0].length;
      const elsewhere = elsewhereOf(match[0], this.workspaceId);
      if (elsewhere) this.add({ kind: "elsewhere", from, to, link: elsewhere });
      else this.add({ kind: "page", from, to, pageId: match[2] });
    }
    for (const match of text.matchAll(BARE_MESSAGE_LINK)) {
      const from = range.from + match.index;
      if (isInside(top, from, NOT_A_CHIP)) continue;
      const to = from + match[0].length;
      const elsewhere = elsewhereOf(match[0], this.workspaceId);
      if (elsewhere) this.add({ kind: "elsewhere", from, to, link: elsewhere });
      else this.add({ kind: "message", from, to, messageId: match[2], label: BARE_MESSAGE_LABEL });
    }
  }

  private hideHeaderMark(mark: SyntaxNode) {
    const heading = mark.parent;
    if (!heading?.name.startsWith("ATXHeading")) return;
    if (mark.from === heading.from) {
      this.hide(mark.from, Math.min(this.afterSpaces(mark.to), heading.to));
    } else {
      this.hide(this.beforeSpaces(mark.from), mark.to);
    }
  }

  private addListMark(mark: SyntaxNode) {
    const list = mark.parent?.parent;
    if (!list) return;
    if (list.name === "OrderedList") {
      this.add({ kind: "style", from: mark.from, to: mark.to, className: "cm-md-list-number" });
      return;
    }
    const line = this.doc.lineAt(mark.from);
    if (mark.from > line.from && !line.text.slice(0, mark.from - line.from).trim()) {
      // Nested items: monospace indentation makes each level visibly deeper.
      this.add({ kind: "style", from: line.from, to: mark.from, className: "cm-md-indent" });
    }
    if (this.isRevealed(mark.from)) {
      this.add({ kind: "style", from: mark.from, to: mark.to, className: "cm-md-list-mark" });
      return;
    }
    const task = mark.parent?.getChild("Task");
    const marker = task?.getChild("TaskMarker");
    if (task && marker && marker.from <= line.to) {
      const label = plainText(this.doc.sliceString(marker.to, Math.min(task.to, line.to)));
      const isTaskChecked = isChecked(this.doc.sliceString(marker.from, marker.to));
      this.add({ kind: "checkbox", from: mark.from, to: marker.to, isChecked: isTaskChecked, label });
      return;
    }
    this.add({ kind: "bullet", from: mark.from, to: mark.to, depth: depthIn(mark, "BulletList") - 1 });
  }

  private addImage(image: SyntaxNode) {
    const url = image.getChild("URL");
    const marks = image.getChildren("LinkMark");
    if (!url || marks.length < 2) return;
    if (this.doc.lineAt(image.from).number !== this.doc.lineAt(image.to).number) return;
    const parsed = parseFileImageUrl(this.doc.sliceString(url.from, url.to));
    if (!parsed) return;
    const caption = unescapeLinkText(this.doc.sliceString(marks[0].to, marks[1].from)).trim();
    this.add({ kind: "image", from: image.from, to: image.to, ...parsed, caption });
  }

  private addLink(link: SyntaxNode): false | undefined {
    const url = link.getChild("URL");
    const marks = link.getChildren("LinkMark");
    if (!url || marks.length < 2) return;
    const href = this.doc.sliceString(url.from, url.to);
    const labelFrom = marks[0].to;
    const labelTo = marks[1].from;
    const isOneLine = this.doc.lineAt(link.from).number === this.doc.lineAt(link.to).number;
    const elsewhere = elsewhereOf(href, this.workspaceId);
    if (elsewhere && isOneLine) {
      if (elsewhere.kind === "page" || elsewhere.kind === "message") {
        this.add({ kind: "elsewhere", from: link.from, to: link.to, link: elsewhere });
      } else {
        // A mention of someone in another workspace: its text, plain, never a chip.
        this.add({ kind: "style", from: labelFrom, to: labelTo, className: "cm-md-plain" });
        this.hide(link.from, labelFrom);
        this.hide(labelTo, link.to);
      }
      return false;
    }
    const pageId = pageIdOf(href, this.workspaceId);
    if (pageId && isOneLine) {
      this.add({ kind: "page", from: link.from, to: link.to, pageId });
      return false;
    }
    const fileId = fileIdOf(href);
    if (fileId && isOneLine) {
      const label = unescapeLinkText(this.doc.sliceString(labelFrom, labelTo)).trim();
      this.add({ kind: "file", from: link.from, to: link.to, fileId, label });
      return false;
    }
    const messageId = messageIdOf(href, this.workspaceId);
    if (messageId && isOneLine) {
      const label = plainText(this.doc.sliceString(labelFrom, labelTo)).trim() || BARE_MESSAGE_LABEL;
      this.add({ kind: "message", from: link.from, to: link.to, messageId, label });
      return false;
    }
    if (isMentionLink(href, this.workspaceId) && isOneLine) {
      const label = this.doc.sliceString(labelFrom, labelTo);
      const memberId = mentionedMemberId(href, this.workspaceId);
      this.add({ kind: "mention", from: link.from, to: link.to, label, memberId });
      return false;
    }
    this.add({ kind: "style", from: labelFrom, to: labelTo, className: "cm-md-link" });
    this.hide(link.from, labelFrom);
    this.hide(labelTo, link.to);
    return;
  }

  private styleQuote(quote: SyntaxNode, range: { from: number; to: number }) {
    const firstLine = this.doc.lineAt(quote.from);
    const mark = quote.getChild("QuoteMark");
    const textFrom = mark ? this.afterOneSpace(mark.to) : quote.from;
    const marker = calloutMarker(this.doc.sliceString(textFrom, firstLine.to));
    if (!marker) {
      this.styleLines(quote.from, quote.to, range, () => "cm-md-quote");
      return;
    }
    const last = this.doc.lineAt(quote.to).number;
    this.styleLines(quote.from, quote.to, range, (line) =>
      [
        "cm-md-callout",
        `callout-${marker.kind}`,
        line === firstLine.number && "cm-md-callout-first",
        line === last && "cm-md-callout-last",
      ]
        .filter(Boolean)
        .join(" "),
    );
    if (!this.isRevealed(textFrom)) {
      this.add({ kind: "callout", from: textFrom, to: textFrom + marker.length, callout: marker.kind });
    }
  }

  private styleCodeBlock(block: SyntaxNode, range: { from: number; to: number }) {
    // Fences read as the block's edges: the ``` marks hide and the language stays as a small label.
    for (const mark of block.getChildren("CodeMark")) this.hide(mark.from, mark.to);
    const info = block.getChild("CodeInfo");
    if (info) this.add({ kind: "style", from: info.from, to: info.to, className: "cm-md-code-language" });
    const first = this.doc.lineAt(block.from).number;
    const last = this.doc.lineAt(block.to).number;
    this.styleLines(block.from, block.to, range, (line) =>
      ["cm-md-codeblock", line === first && "cm-md-codeblock-first", line === last && "cm-md-codeblock-last"]
        .filter(Boolean)
        .join(" "),
    );
  }

  /** Styles the lines from `from` to `to` that fall in `range`; huge blocks cost only what's visible. */
  private styleLines(
    from: number,
    to: number,
    range: { from: number; to: number },
    classFor: (line: number) => string,
  ) {
    if (to < from) return;
    const first = this.doc.lineAt(Math.max(from, range.from)).number;
    const last = this.doc.lineAt(Math.min(to, range.to)).number;
    for (let number = first; number <= last; number += 1) {
      this.add({ kind: "line", from: this.doc.line(number).from, className: classFor(number) });
    }
  }

  /** Hides a range unless its line is revealed. Never across a line break: plugins may not replace those. */
  private hide(from: number, to: number) {
    if (to <= from || this.isRevealed(from)) return;
    if (this.doc.lineAt(from).number !== this.doc.lineAt(to).number) return;
    this.add({ kind: "hide", from, to });
  }

  private isRevealed(position: number): boolean {
    return this.revealedLines.has(this.doc.lineAt(position).number);
  }

  private afterSpaces(position: number): number {
    const lineEnd = this.doc.lineAt(position).to;
    let end = position;
    while (end < lineEnd && /[ \t]/.test(this.doc.sliceString(end, end + 1))) end += 1;
    return end;
  }

  private afterOneSpace(position: number): number {
    return this.doc.sliceString(position, position + 1) === " " ? position + 1 : position;
  }

  private beforeSpaces(position: number): number {
    const lineStart = this.doc.lineAt(position).from;
    let start = position;
    while (start > lineStart && /[ \t]/.test(this.doc.sliceString(start - 1, start))) start -= 1;
    return start;
  }

  /** One item per kind and place: overlapping visible ranges may report a node twice. */
  private add(item: PreviewItem) {
    // CodeMirror rejects empty styles (an empty checked item, a link without a label).
    if (item.kind === "style" && item.to <= item.from) return;
    const to = "to" in item ? item.to : item.from;
    const key = item.kind === "line" ? `line:${item.from}:${item.className}` : `${item.kind}:${item.from}:${to}`;
    this.found.set(key, item);
  }
}

function isChecked(marker: string): boolean {
  return /x/i.test(marker);
}

function depthIn(node: SyntaxNode, name: string): number {
  let depth = 0;
  for (let ancestor = node.parent; ancestor; ancestor = ancestor.parent) if (ancestor.name === name) depth += 1;
  return depth;
}

function isInside(top: SyntaxNode, position: number, names: ReadonlySet<string>): boolean {
  for (let node: SyntaxNode | null = top.resolveInner(position, 1); node; node = node.parent) {
    if (names.has(node.name)) return true;
  }
  return false;
}

/** A task's text without markdown marks, for its checkbox's accessible name. */
function plainText(markdown: string): string {
  return markdown
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[*_~`]/g, "")
    .trim();
}
