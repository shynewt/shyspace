import type * as Y from "yjs";
import type { SearchHit } from "../../../src/lib/api";
import { rootOf } from "./threads";

const HIGHLIGHT_START = "\u0001";
const HIGHLIGHT_END = "\u0002";

type Searchable = {
  messages: readonly { id: string; channel: string; text: string; reply_to: string | null; author: string }[];
  channels: readonly { id: string; name: string }[];
  pages: readonly { id: string; title: string; doc: Y.Doc }[];
};

/** Like the core: every term must start a word; matches are marked. */
export function search(query: string, { messages, channels, pages }: Searchable): SearchHit[] {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [];
  const matches = (text: string) =>
    terms.every((term) =>
      text
        .toLowerCase()
        .split(/\W+/)
        .some((word) => word.startsWith(term)),
    );
  const hits: SearchHit[] = [];
  for (const message of messages) {
    if (!matches(message.text)) continue;
    const channel = channels.find((candidate) => candidate.id === message.channel);
    hits.push({
      kind: "message",
      id: message.id,
      title: channel?.name ?? "",
      snippet: highlight(message.text, terms),
      channel: message.channel,
      thread_root: rootOf(messages, message.id) === message.id ? null : rootOf(messages, message.id),
    });
  }
  for (const page of pages) {
    const body = page.doc.getText("content").toString();
    if (!matches(`${page.title} ${body}`)) continue;
    hits.push({
      kind: "page",
      id: page.id,
      title: page.title,
      snippet: highlight(body, terms),
      channel: null,
      thread_root: null,
    });
  }
  return hits;
}

function highlight(text: string, terms: string[]): string {
  return text.replace(/\w+/g, (word) =>
    terms.some((term) => word.toLowerCase().startsWith(term)) ? `${HIGHLIGHT_START}${word}${HIGHLIGHT_END}` : word,
  );
}
