import { openUrl } from "@tauri-apps/plugin-opener";
import Markdown, { type Components } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { PAGE_LINK_PREFIX } from "./api";
import { isMentionLink, linkMentions } from "./mentions";
import { reportError } from "./toast";

/** The id part of `shy://page/<id>` and `shy://message/<id>` links. */
const PAGE_ID = "[0-9a-f]+";
const MESSAGE_ID = "[0-9a-f]+";

const PAGE_LINK = new RegExp(`^shy://page/(${PAGE_ID})$`);
const MESSAGE_LINK = new RegExp(`^shy://message/(${MESSAGE_ID})$`);
const ANY_PAGE_LINK = new RegExp(`shy://page/(${PAGE_ID})`, "g");
const ANY_MESSAGE_LINK = new RegExp(`shy://message/(${MESSAGE_ID})`, "g");
const BARE_PAGE_LINK = new RegExp(`(?<!\\]\\()shy://page/(${PAGE_ID})`, "g");
const BARE_MESSAGE_LINK = new RegExp(`(?<!\\]\\()shy://message/(${MESSAGE_ID})`, "g");
const SAFE_EXTERNAL = /^(https?:|mailto:)/i;

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
}: {
  text: string;
  /** Mentions of this member stand out. */
  myId: string;
  pageTitle: (id: string) => string | undefined;
  onOpenPage: (id: string) => void;
}) {
  const withTitles = text
    .replace(BARE_PAGE_LINK, (_, id: string) => {
      const title = (pageTitle(id) ?? "Untitled page").replace(/[[\]]/g, "");
      return `[${title}](${PAGE_LINK_PREFIX}${id})`;
    })
    .replace(BARE_MESSAGE_LINK, (link: string) => `[a message](${link})`);
  const components: Components = {
    a: ({ href = "", children }) => {
      if (MESSAGE_LINK.test(href)) {
        // The linked message shows as a card under the text.
        return <span className="page-chip">{children}</span>;
      }
      if (isMentionLink(href)) {
        return <span className={linkMentions(href, myId) ? "mention mention-me" : "mention"}>{children}</span>;
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
        <a
          href={href}
          onClick={(event) => {
            event.preventDefault();
            openUrl(href).catch(reportError);
          }}
        >
          {children}
        </a>
      );
    },
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
