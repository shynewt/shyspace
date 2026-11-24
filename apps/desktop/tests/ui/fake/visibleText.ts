/**
 * Like the core's visible text for previews and snippets: one line, markdown
 * marks dropped, and app links named as the chips name them. Plain spellings
 * only: the core's own tests cover the rest.
 */

export type LinkNames = {
  member: (id: string) => string | undefined;
  page: (id: string) => string | undefined;
};

export function visibleText(text: string, names: LinkNames): string {
  return text
    .replace(/\[[^\]]*\]\(shy:\/\/member\/([0-9a-f]+)\)/g, (_, id: string) => `@${memberName(id, names)}`)
    .replace(/\[[^\]]*\]\(shy:\/\/mention\/(channel|here)\)/g, "@$1")
    .replace(/\[[^\]]*\]\((shy:\/\/(?:page|message)\/[^)\s]+)\)/g, "$1")
    .replace(/shy:\/\/page\/([0-9a-f]+)/g, (_, id: string) => `📄 ${names.page(id) || "Untitled page"}`)
    .replace(/shy:\/\/message\/[^\s)]+/g, "💬 a message")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s*(?:#{1,6}|>|[-*+])\s+/gm, "")
    .replace(/\*\*|__|`/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function memberName(id: string, names: LinkNames): string {
  return names.member(id)?.trim() || `member ${id.slice(0, 6)}`;
}
