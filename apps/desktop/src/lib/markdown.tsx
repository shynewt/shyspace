import { openUrl } from "@tauri-apps/plugin-opener";
import Markdown, { type Components } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";
import { PAGE_LINK_PREFIX } from "./api";
import { reportError } from "./toast";

const BARE_PAGE_LINK = /(?<!\]\()shy:\/\/page\/([0-9a-f]+)/g;
const PAGE_LINK = /^shy:\/\/page\/([0-9a-f]+)$/;
const SAFE_EXTERNAL = /^(https?:|mailto:)/i;

/**
 * Renders a chat message. Messages come from other people, so this never
 * injects HTML: markdown becomes React elements, raw HTML is dropped, and
 * only web, mail and page links survive.
 */
export function ChatMarkdown({
  text,
  pageTitle,
  onOpenPage,
}: {
  text: string;
  pageTitle: (id: string) => string | undefined;
  onOpenPage: (id: string) => void;
}) {
  const withTitles = text.replace(BARE_PAGE_LINK, (_, id: string) => {
    const title = (pageTitle(id) ?? "Untitled page").replace(/[[\]]/g, "");
    return `[${title}](${PAGE_LINK_PREFIX}${id})`;
  });
  const components: Components = {
    a: ({ href = "", children }) => {
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
      urlTransform={(url) => (SAFE_EXTERNAL.test(url) || PAGE_LINK.test(url) ? url : "")}
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

/** One-line text for quotes and previews: page links shown by title. */
export function plainPreview(text: string, pageTitle: (id: string) => string | undefined): string {
  return text
    .replace(/shy:\/\/page\/([0-9a-f]+)/g, (_, id: string) => pageTitle(id) ?? "Untitled page")
    .replace(/\s+/g, " ");
}
