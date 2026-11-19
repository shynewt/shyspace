import DOMPurify from "dompurify";
import { marked } from "marked";
import { PAGE_LINK_PREFIX } from "./api";

marked.setOptions({ gfm: true, breaks: true });

const BARE_PAGE_LINK = /(?<!\]\()shy:\/\/page\/([0-9a-f]+)/g;
const ALLOWED_URI = /^(?:https?:|mailto:|shy:\/\/page\/)/i;

/**
 * Renders a chat message. Messages come from other people, so the HTML is
 * sanitized; page links become chips titled with the page's current name.
 */
export function renderMarkdown(text: string, pageTitle: (id: string) => string | undefined): string {
  const linked = text.replace(BARE_PAGE_LINK, (_, id: string) => {
    const title = (pageTitle(id) ?? "Untitled page").replace(/[[\]]/g, "");
    return `[${title}](${PAGE_LINK_PREFIX}${id})`;
  });
  const html = marked.parse(linked, { async: false });
  return DOMPurify.sanitize(html, {
    ALLOWED_URI_REGEXP: ALLOWED_URI,
    FORBID_TAGS: ["img", "style", "iframe", "form", "input"],
    FORBID_ATTR: ["style"],
  }).replace(/<a href="shy:\/\/page\/([0-9a-f]+)"/g, '<a href="shy://page/$1" class="page-chip" data-page="$1"');
}

export function pageIdsIn(text: string): string[] {
  return [...new Set([...text.matchAll(/shy:\/\/page\/([0-9a-f]+)/g)].map((match) => match[1]))];
}

/** One-line text for quotes and previews: page links shown by title. */
export function plainPreview(text: string, pageTitle: (id: string) => string | undefined): string {
  return text.replace(/shy:\/\/page\/([0-9a-f]+)/g, (_, id: string) => pageTitle(id) ?? "Untitled page").replace(/\s+/g, " ");
}
