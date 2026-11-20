import { openUrl } from "@tauri-apps/plugin-opener";
import Markdown, { type Components } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { PAGE_LINK_PREFIX } from "./api";
import { isMentionLink, linkMentions } from "./mentions";
import { reportError } from "./toast";

const BARE_PAGE_LINK = /(?<!\]\()shy:\/\/page\/([0-9a-f]+)/g;
const PAGE_LINK = /^shy:\/\/page\/([0-9a-f]+)$/;
const MESSAGE_LINK = /^shy:\/\/message\/([0-9a-f]+)$/;
const BARE_MESSAGE_LINK = /(?<!\]\()shy:\/\/message\/([0-9a-f]+)/g;
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
  return [...new Set([...text.matchAll(/shy:\/\/page\/([0-9a-f]+)/g)].map((match) => match[1]))];
}

/** One-line text for quotes and previews: page links shown by title, message links in words, mentions as @name. */
export function plainPreview(text: string, pageTitle: (id: string) => string | undefined): string {
  return text
    .replace(/\[(@[^\]]*)\]\(shy:\/\/(?:member|mention)\/[0-9a-z]+\)/g, "$1")
    .replace(/shy:\/\/page\/([0-9a-f]+)/g, (_, id: string) => pageTitle(id) ?? "Untitled page")
    .replace(/\[([^\]]*)\]\(shy:\/\/message\/[0-9a-f]+\)/g, "$1")
    .replace(/shy:\/\/message\/[0-9a-f]+/g, "a linked message")
    .replace(/\s+/g, " ");
}

export function messageIdsIn(text: string): string[] {
  return [...new Set([...text.matchAll(/shy:\/\/message\/([0-9a-f]+)/g)].map((match) => match[1]))];
}
