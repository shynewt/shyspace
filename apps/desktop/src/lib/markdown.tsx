import { Check, ChevronDown, ChevronUp, Copy, FileText, Quote } from "lucide-react";
import {
  type ComponentProps,
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import Markdown, { type Components, type ExtraProps } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { Avatar, ConfirmDialog, cx, ErrorBoundary, IconButton, Tooltip } from "../ui";
import { type AppLink, appLinkPattern, isInWorkspace, parseAppLink, type WorkspaceLinks } from "./appLinks";
import { CALLOUTS, type CalloutKind, isCalloutKind, rehypeCallouts } from "./callouts";
import { type CodeSegment, highlight } from "./codeHighlight";
import { isMisleadingLink, misleadingLinkWarning, openExternalUrl } from "./contact";
import { useWorkspaceIcon } from "./files";
import { isCheapToFormat, remarkSizeLimits, withPlainLineEnds } from "./markdownBudget";
import { everyoneMentioned, isMentionLink, linkMentions, mentionedMemberId } from "./mentions";
import { displayName } from "./people";
import { reportError } from "./toast";

const ANY_PAGE_LINK = appLinkPattern("page");
const ANY_MESSAGE_LINK = appLinkPattern("message");
const BARE_PAGE_LINK = new RegExp(`(?<!\\]\\()${ANY_PAGE_LINK.source}`, "g");
const BARE_MESSAGE_LINK = new RegExp(`(?<!\\]\\()${ANY_MESSAGE_LINK.source}`, "g");
/** A link alone on its line, which a card under the message can stand in for. */
const APP_LINK_LINE = new RegExp(
  `^[ \\t]*((?:${ANY_PAGE_LINK.source})|(?:${ANY_MESSAGE_LINK.source}))[ \\t]*(?:\\n|$)`,
  "gm",
);
const SAFE_EXTERNAL = /^(https?:|mailto:)/i;
const NO_CARDS: ReadonlySet<string> = new Set();

const CODE_MAX_HEIGHT_PX = 240;
const COPIED_FEEDBACK_MS = 1400;

type MarkdownNode = NonNullable<ExtraProps["node"]>;

/**
 * Renders a chat message. Messages come from other people, so this never
 * injects HTML: markdown becomes React elements, raw HTML is dropped, and
 * only web, mail, page and mention links survive. Text too costly to format,
 * or that fails to draw, shows as plain text.
 */
export function ChatMarkdown({
  text,
  myId,
  pageTitle,
  memberName,
  onOpenPage,
  onOpenMessage,
  renderMemberMention,
  workspaceLinks,
  cardLinks = NO_CARDS,
}: {
  text: string;
  /** Mentions of this member stand out. */
  myId: string;
  pageTitle: (id: string) => string | undefined;
  /** A member's name as the app shows it (with a safety code when names collide); a mention chip draws this. */
  memberName: (id: string) => string | undefined;
  onOpenPage: (id: string) => void;
  onOpenMessage: (id: string) => void;
  /** Draws a mention of a member, like a button opening their card; plain text without it. */
  renderMemberMention?: (memberId: string, label: ReactNode, className: string) => ReactNode;
  /** The workspace this text is in, and the others on this device, for links that name one. */
  workspaceLinks: WorkspaceLinks;
  /** Links drawn as cards under the text (see `cardLinkKey`); one alone on its line leaves the text. */
  cardLinks?: ReadonlySet<string>;
}) {
  // A bare page link gets an empty label, which the link fills with the page's
  // title as it draws: titles come from other members, so one never goes into
  // the markdown source, where `<https://…>`, a backtick or a final `\` would
  // be read as markup.
  const source = withPlainLineEnds(text)
    .replace(APP_LINK_LINE, (line: string, link: string) =>
      cardLinks.has(cardKey(link, workspaceLinks.workspaceId) ?? "") ? "" : line,
    )
    .replace(BARE_PAGE_LINK, (link: string) => `[](${link})`)
    .replace(BARE_MESSAGE_LINK, (link: string) => `[a message](${link})`);
  // Checked as the parser will see it.
  if (!isCheapToFormat(source)) return <PlainText text={text} />;
  return (
    <ErrorBoundary key={text} fallback={<PlainText text={text} />}>
      <LinkContext.Provider
        value={{ myId, pageTitle, memberName, onOpenPage, onOpenMessage, renderMemberMention, workspaceLinks }}
      >
        <Markdown
          remarkPlugins={[remarkGfm, remarkBreaks, remarkSizeLimits]}
          rehypePlugins={[rehypeCallouts]}
          skipHtml
          urlTransform={(url) => (SAFE_EXTERNAL.test(url) || parseAppLink(url) ? url : "")}
          disallowedElements={["img"]}
          components={COMPONENTS}
        >
          {source}
        </Markdown>
      </LinkContext.Provider>
    </ErrorBoundary>
  );
}

function PlainText({ text }: { text: string }) {
  return <p className="whitespace-pre-wrap break-words">{text}</p>;
}

type LinkHandlers = {
  myId: string;
  pageTitle: (id: string) => string | undefined;
  memberName: (id: string) => string | undefined;
  onOpenPage: (id: string) => void;
  onOpenMessage: (id: string) => void;
  renderMemberMention?: (memberId: string, label: ReactNode, className: string) => ReactNode;
  workspaceLinks: WorkspaceLinks;
};

const LinkContext = createContext<LinkHandlers | null>(null);

function MarkdownLink({ href = "", children, node }: ComponentProps<"a"> & ExtraProps) {
  const handlers = useContext(LinkContext);
  if (!handlers) throw new Error("a markdown link outside ChatMarkdown");
  const { myId, pageTitle, memberName, onOpenPage, onOpenMessage, renderMemberMention, workspaceLinks } = handlers;
  const { workspaceId } = workspaceLinks;
  // An address the renderer blocked (javascript: and the like) arrives empty: show what it said, not a link to nowhere.
  if (href === "") return <>{children}</>;
  // Chips name what they open, as plain text the app knows, never the author's
  // label: "@Ada" leading to Eve, or "Payroll" opening another page, would lie.
  const link = parseAppLink(href);
  if (link && !isInWorkspace(link, workspaceId)) {
    return (
      <LinkElsewhere link={link} workspaceLinks={workspaceLinks}>
        {children}
      </LinkElsewhere>
    );
  }
  const message = link?.kind === "message" ? link.id : null;
  if (message) {
    return (
      <button type="button" className="page-chip" onClick={() => onOpenMessage(message)}>
        a message
      </button>
    );
  }
  if (isMentionLink(href, workspaceId)) {
    const className = linkMentions(href, myId, workspaceId) ? "mention mention-me" : "mention";
    const memberId = mentionedMemberId(href, workspaceId);
    if (!memberId) return <span className={className}>@{everyoneMentioned(href, workspaceId)}</span>;
    const name = `@${memberName(memberId) ?? displayName("", memberId)}`;
    if (renderMemberMention) return renderMemberMention(memberId, name, className);
    return <span className={className}>{name}</span>;
  }
  const page = link?.kind === "page" ? link.id : null;
  if (page) {
    return (
      <button type="button" className="page-chip" onClick={() => onOpenPage(page)}>
        {pageTitle(page) ?? "Untitled page"}
      </button>
    );
  }
  const label = node ? textOf(node) : "";
  return (
    <ExternalLink href={href} text={label}>
      {children}
    </ExternalLink>
  );
}

/**
 * Module-level, so each render gets the same component types: new ones would
 * make React rebuild those elements, losing their state (an opened code block)
 * and briefly shrinking the message, which moves a scrolled list in WebKit.
 */
const COMPONENTS: Components = {
  a: MarkdownLink,
  pre: ({ children, node }) => (
    <CodeBlock language={node ? languageOf(node) : ""} code={node ? textOf(node) : ""}>
      {children}
    </CodeBlock>
  ),
  table: ({ children }) => (
    <div className="table-scroll">
      <table>{children}</table>
    </div>
  ),
  aside: ({ children, node }) => {
    const kind = node?.properties.dataCallout;
    return isCalloutKind(kind) ? <Callout kind={kind}>{children}</Callout> : <aside>{children}</aside>;
  },
};

/** The language a fenced block names (```ts), from its `language-ts` class. */
function languageOf(pre: MarkdownNode): string {
  const code = pre.children.find((child) => child.type === "element" && child.tagName === "code");
  const classes = code?.type === "element" ? code.properties.className : undefined;
  const name = Array.isArray(classes) ? classes.find((name) => String(name).startsWith("language-")) : undefined;
  return name === undefined ? "" : String(name).slice("language-".length);
}

function textOf(node: MarkdownNode | MarkdownNode["children"][number]): string {
  if (node.type === "text") return node.value;
  if (node.type === "element") return node.children.map(textOf).join("");
  return "";
}

/** A web or mail link: shows where it goes on hover, and asks first when its text shows somewhere else. */
function ExternalLink({ href, text, children }: { href: string; text: string; children: ReactNode }) {
  const [isConfirming, setConfirming] = useState(false);
  const open = () => openExternalUrl(href);
  return (
    <>
      <Tooltip label={href}>
        <a
          href={href}
          onClick={(event) => {
            event.preventDefault();
            if (isMisleadingLink(text, href)) setConfirming(true);
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
        description={misleadingLinkWarning(text, href)}
        confirmLabel="Open link"
        onConfirm={open}
      />
    </>
  );
}

function Callout({ kind, children }: { kind: CalloutKind; children: ReactNode }) {
  const { label, iconPaths } = CALLOUTS[kind];
  return (
    <div role="note" className={`callout callout-${kind}`}>
      <span className="callout-title">
        <svg
          viewBox="0 0 24 24"
          width="1em"
          height="1em"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          {iconPaths.map((path) => (
            <path key={path} d={path} />
          ))}
        </svg>
        {label}
      </span>
      {children}
    </div>
  );
}

/** A code block with a copy button and syntax colours; a tall one folds to a few lines until opened. */
function CodeBlock({ language, code, children }: { language: string; code: string; children: ReactNode }) {
  const pre = useRef<HTMLPreElement>(null);
  const segments = useHighlighted(code, language);
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
          {segments ? (
            <code>
              {segments.map((segment) =>
                segment.className ? (
                  <span key={segment.from} className={segment.className}>
                    {segment.text}
                  </span>
                ) : (
                  segment.text
                ),
              )}
            </code>
          ) : (
            children
          )}
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

/** Coloured runs of `code` once its language has loaded; null before, or for unknown languages. */
function useHighlighted(code: string, language: string): CodeSegment[] | null {
  const [highlighted, setHighlighted] = useState<{ code: string; segments: CodeSegment[] | null } | null>(null);
  useEffect(() => {
    let isCurrent = true;
    highlight(code, language).then((segments) => {
      if (isCurrent) setHighlighted({ code, segments });
    }, reportError);
    return () => {
      isCurrent = false;
    };
  }, [code, language]);
  return highlighted?.code === code ? highlighted.segments : null;
}

/** The pages `text` links to in `workspaceId`. */
export function pageIdsIn(text: string, workspaceId: string): string[] {
  return idsHere(text.matchAll(ANY_PAGE_LINK), workspaceId);
}

/** The messages `text` links to in `workspaceId`. */
export function messageIdsIn(text: string, workspaceId: string): string[] {
  return idsHere(text.matchAll(ANY_MESSAGE_LINK), workspaceId);
}

function idsHere(matches: Iterable<RegExpMatchArray>, workspaceId: string): string[] {
  const here = [...matches].filter((match) => isInWorkspace({ workspace: match[1] ?? null }, workspaceId));
  return [...new Set(here.map((match) => match[2]))];
}

/** How a page or message of this workspace is named in `cardLinks`. */
export function cardLinkKey(kind: "page" | "message", id: string): string {
  return `${kind}:${id}`;
}

/** A link's key in `cardLinks`, whichever form it's written in; null for one into another workspace. */
function cardKey(link: string, workspaceId: string): string | null {
  const parsed = parseAppLink(link);
  if (!parsed || !isInWorkspace(parsed, workspaceId)) return null;
  return parsed.kind === "page" || parsed.kind === "message" ? cardLinkKey(parsed.kind, parsed.id) : null;
}

/**
 * A link into another workspace. On this device: a chip naming the kind and
 * the workspace, opening it there. Elsewhere: quiet words, nothing to click.
 * A mention of someone there is its text, never a chip: its label is the
 * author's, and a chip would vouch for it.
 */
function LinkElsewhere({
  link,
  workspaceLinks,
  children,
}: {
  link: AppLink;
  workspaceLinks: WorkspaceLinks;
  children: ReactNode;
}) {
  if (link.kind !== "page" && link.kind !== "message") return <>{children}</>;
  const workspace = link.workspace ? workspaceLinks.workspaceOf(link.workspace) : undefined;
  if (!link.workspace || !workspace) return <span className="text-muted">link to a workspace you're not in</span>;
  const { name } = workspace;
  const Icon = link.kind === "page" ? FileText : Quote;
  const kind = link.kind === "page" ? "Page" : "Message";
  const description = `Open the ${link.kind} in ${name}`;
  return (
    <Tooltip label={description}>
      <button
        type="button"
        className="page-chip inline-flex items-center gap-1 align-bottom"
        aria-label={description}
        onClick={() => workspaceLinks.openElsewhere(link)}
      >
        <Icon size={12} aria-hidden className="shrink-0" />
        <span className="shrink-0">{kind} in</span>
        <WorkspaceTile workspaceId={link.workspace} name={name} icon={workspace.icon} />
        <span className="max-w-40 truncate">{name}</span>
      </button>
    </Tooltip>
  );
}

/** A workspace's rail icon, small, so a chip shows its click leaves this workspace. */
function WorkspaceTile({ workspaceId, name, icon }: { workspaceId: string; name: string; icon: string | null }) {
  const pictureUrl = useWorkspaceIcon(workspaceId, icon);
  return <Avatar id={workspaceId} name={name} size={14} pictureUrl={pictureUrl} />;
}
