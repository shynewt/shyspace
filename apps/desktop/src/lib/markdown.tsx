import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, ChevronDown, ChevronUp, Copy } from "lucide-react";
import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import Markdown, { type Components, type ExtraProps } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { ConfirmDialog, cx, IconButton, Tooltip } from "../ui";
import { PAGE_LINK_PREFIX } from "./api";
import { isMentionLink, linkMentions, mentionedMemberId } from "./mentions";
import { reportError } from "./toast";

/** The id part of `shy://page/<id>` and `shy://message/<id>` links. */
const PAGE_ID = "[0-9a-f]+";
/** A message id; in a private channel or DM it starts with the channel, like `p:…/…`. */
const MESSAGE_ID = "(?:(?:p|dm):[0-9a-f]+/)?[0-9a-f]+";

const PAGE_LINK = new RegExp(`^shy://page/(${PAGE_ID})$`);
const MESSAGE_LINK = new RegExp(`^shy://message/(${MESSAGE_ID})$`);
const ANY_PAGE_LINK = new RegExp(`shy://page/(${PAGE_ID})`, "g");
const ANY_MESSAGE_LINK = new RegExp(`shy://message/(${MESSAGE_ID})`, "g");
const BARE_PAGE_LINK = new RegExp(`(?<!\\]\\()shy://page/(${PAGE_ID})`, "g");
const BARE_MESSAGE_LINK = new RegExp(`(?<!\\]\\()shy://message/(${MESSAGE_ID})`, "g");
/** A link alone on its line: the card under the message shows it instead. */
const PAGE_LINK_LINE = new RegExp(`^[ \\t]*shy://page/(${PAGE_ID})[ \\t]*(?:\\n|$)`, "gm");
const MESSAGE_LINK_LINE = new RegExp(`^[ \\t]*shy://message/${MESSAGE_ID}[ \\t]*(?:\\n|$)`, "gm");
const SAFE_EXTERNAL = /^(https?:|mailto:)/i;

/** Previews show a line or two; reading further into a long (or hostile) message only costs time. */
const PREVIEW_SOURCE_CHARS = 1000;
const CODE_MAX_HEIGHT_PX = 240;
const COPIED_FEEDBACK_MS = 1400;

type MarkdownNode = NonNullable<ExtraProps["node"]>;

/**
 * Renders a chat message. Messages come from other people, so this never
 * injects HTML: markdown becomes React elements, raw HTML is dropped, and
 * only web, mail, page and mention links survive.
 */
export function ChatMarkdown({
  text,
  myId,
  pageTitle,
  onOpenPage,
  onOpenMessage,
  renderMemberMention,
}: {
  text: string;
  /** Mentions of this member stand out. */
  myId: string;
  pageTitle: (id: string) => string | undefined;
  onOpenPage: (id: string) => void;
  onOpenMessage: (id: string) => void;
  /** Draws a mention of a member, like a button opening their card; plain text without it. */
  renderMemberMention?: (memberId: string, label: ReactNode, className: string) => ReactNode;
}) {
  const withTitles = text
    .replace(PAGE_LINK_LINE, (line: string, id: string) => (pageTitle(id) === undefined ? line : ""))
    .replace(MESSAGE_LINK_LINE, "")
    .replace(BARE_PAGE_LINK, (_, id: string) => {
      const title = (pageTitle(id) ?? "Untitled page").replace(/[[\]]/g, "");
      return `[${title}](${PAGE_LINK_PREFIX}${id})`;
    })
    .replace(BARE_MESSAGE_LINK, (link: string) => `[a message](${link})`);
  const components: Components = {
    a: ({ href = "", children, node }) => {
      const message = href.match(MESSAGE_LINK)?.[1];
      if (message) {
        return (
          <button type="button" className="page-chip" onClick={() => onOpenMessage(message)}>
            {children}
          </button>
        );
      }
      if (isMentionLink(href)) {
        const className = linkMentions(href, myId) ? "mention mention-me" : "mention";
        const memberId = mentionedMemberId(href);
        if (memberId && renderMemberMention) return renderMemberMention(memberId, children, className);
        return <span className={className}>{children}</span>;
      }
      const page = href.match(PAGE_LINK)?.[1];
      if (page) {
        return (
          <button type="button" className="page-chip" onClick={() => onOpenPage(page)}>
            {children}
          </button>
        );
      }
      return (
        <ExternalLink href={href} text={node ? textOf(node) : ""}>
          {children}
        </ExternalLink>
      );
    },
    pre: ({ children }) => <CodeBlock>{children}</CodeBlock>,
  };
  return (
    <Markdown
      remarkPlugins={[remarkGfm, remarkBreaks]}
      skipHtml
      urlTransform={(url) =>
        SAFE_EXTERNAL.test(url) || PAGE_LINK.test(url) || MESSAGE_LINK.test(url) || isMentionLink(url) ? url : ""
      }
      disallowedElements={["img"]}
      components={components}
    >
      {withTitles}
    </Markdown>
  );
}

function textOf(node: MarkdownNode | MarkdownNode["children"][number]): string {
  if (node.type === "text") return node.value;
  if (node.type === "element") return node.children.map(textOf).join("");
  return "";
}

/** One word that reads as a web address, in any script: "paypal.com", "https://bank.example/login". */
const ADDRESS_WORD = /^(?:https?:\/\/)?(?:[\p{L}\p{N}-]+\.)+\p{L}{2,}(?:[:/?#]\S*)?$/iu;
const WORD_EDGE_PUNCTUATION = /^[("'«“]+|[.,;:!?)"'»”]+$/gu;

/** A URL's host as the system opens it: look-alike letters in punycode, no "www.", and null if it isn't a URL. */
function hostOf(url: string): string | null {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    // Not an address at all; callers treat an unknown host as one to ask about.
    return null;
  }
}

/** The hosts of the addresses written in a link's text, like "Log in at paypal.com." */
function shownHosts(text: string): (string | null)[] {
  return text
    .split(/\s+/)
    .map((word) => word.replace(WORD_EDGE_PUNCTUATION, ""))
    .filter((word) => ADDRESS_WORD.test(word))
    .map((word) => hostOf(/^https?:\/\//i.test(word) ? word : `http://${word}`));
}

/** Link text that shows a web address other than the one the link opens: a common phishing trick. */
function isMisleading(text: string, href: string): boolean {
  const opened = hostOf(href);
  return shownHosts(text).some((shown) => shown === null || shown !== opened);
}

/** A web or mail link: shows where it goes on hover, and asks first when its text shows somewhere else. */
function ExternalLink({ href, text, children }: { href: string; text: string; children: ReactNode }) {
  const [isConfirming, setConfirming] = useState(false);
  const open = () => openUrl(href);
  return (
    <>
      <Tooltip label={<span className="block max-w-[360px] break-all whitespace-normal">{href}</span>}>
        <a
          href={href}
          onClick={(event) => {
            event.preventDefault();
            if (isMisleading(text, href)) setConfirming(true);
            else open().catch(reportError);
          }}
        >
          {children}
        </a>
      </Tooltip>
      <ConfirmDialog
        open={isConfirming}
        onOpenChange={setConfirming}
        title="Open this link?"
        description={`Its text shows ${text.trim()}, but it opens ${href}. Open it only if you trust where it goes.`}
        confirmLabel="Open link"
        onConfirm={open}
      />
    </>
  );
}

/** A code block with a copy button; a tall one folds to a few lines until opened. */
function CodeBlock({ children }: { children: ReactNode }) {
  const pre = useRef<HTMLPreElement>(null);
  const [isTall, setTall] = useState(false);
  const [isExpanded, setExpanded] = useState(false);
  const [isCopied, setCopied] = useState(false);

  useLayoutEffect(() => {
    if (pre.current) setTall(pre.current.scrollHeight > CODE_MAX_HEIGHT_PX);
  });

  useEffect(() => {
    if (!isCopied) return;
    const timer = setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS);
    return () => clearTimeout(timer);
  }, [isCopied]);

  const copy = () => {
    const code = pre.current?.textContent;
    if (code === null || code === undefined) return;
    // Markdown ends a code block's text with a line break that isn't part of the code.
    navigator.clipboard.writeText(code.replace(/\n$/, "")).then(() => setCopied(true), reportError);
  };
  const isFolded = isTall && !isExpanded;

  return (
    <>
      <div className="group/code relative">
        <pre
          ref={pre}
          style={isFolded ? { maxHeight: CODE_MAX_HEIGHT_PX } : undefined}
          className={cx(isFolded && "overflow-y-hidden")}
        >
          {children}
        </pre>
        {isFolded && (
          <div className="pointer-events-none absolute inset-x-px bottom-px h-14 rounded-b-lg bg-linear-to-t from-surface-2 to-transparent" />
        )}
        <span className="absolute right-1.5 bottom-1.5 opacity-0 transition-opacity group-hover/code:opacity-100 keyboard-focus-within:opacity-100">
          <IconButton
            label={isCopied ? "Copied" : "Copy code"}
            size="sm"
            icon={isCopied ? <Check size={13} className="text-success" /> : <Copy size={13} />}
            onClick={copy}
            className="bg-surface shadow-card"
          />
        </span>
      </div>
      {isTall && (
        <button
          type="button"
          aria-expanded={isExpanded}
          onClick={() => setExpanded(!isExpanded)}
          className="mt-1 flex items-center gap-1 rounded-md text-xs font-medium text-accent-text hover:underline"
        >
          {isExpanded ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
          {isExpanded ? "Show less" : "Show more"}
        </button>
      )}
    </>
  );
}

export function pageIdsIn(text: string): string[] {
  return [...new Set([...text.matchAll(ANY_PAGE_LINK)].map((match) => match[1]))];
}

export function messageIdsIn(text: string): string[] {
  return [...new Set([...text.matchAll(ANY_MESSAGE_LINK)].map((match) => match[1]))];
}

/**
 * One line of plain words for quotes, previews and search results: markdown
 * marks dropped, links as their text, page links by title, lines joined with
 * " · ". Copes with text cut off inside a link, as search snippets can be.
 */
export function plainPreview(text: string, pageTitle: (id: string) => string | undefined): string {
  return text
    .slice(0, PREVIEW_SOURCE_CHARS)
    .replace(/[\uD800-\uDBFF]$/, "")
    .replace(/!?\[([^\]]*)\]\([^)\s]*\)/g, "$1")
    .replace(/\]\([^)\s]*\)/g, "")
    .replace(/\[([^\]]*)\]\([^)\s]*?(…?)$/, "$1$2")
    .replace(/\[([^\][]*)$/, "$1")
    .replace(ANY_PAGE_LINK, (_, id: string) => pageTitle(id) ?? "Untitled page")
    .replace(ANY_MESSAGE_LINK, "a linked message")
    .split("\n")
    .map(plainLine)
    .filter(Boolean)
    .join(" · ")
    .replace(/\s+/g, " ");
}

function plainLine(line: string): string {
  return line
    .replace(/^\s*(?:#{1,6}\s+|>\s?|```.*$)/, "")
    .replace(/^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?/, "")
    .replace(/\*\*|__|~~|`/g, "")
    .replace(/(^|\W)[*_](\S(?:.*?\S)?)[*_](?=\W|$)/g, "$1$2")
    .trim();
}
