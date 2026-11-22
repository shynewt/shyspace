import type { Message } from "../../lib/api";

const MENTION_LINK = /\]\(shy:\/\/(?:member|mention)\/[0-9a-z]+\)/;

/**
 * Whether a message you sent is still waiting for an answer: it starts a
 * conversation (a thread reply can't tell), asks something (has a "?") or
 * mentions someone, and nobody else has replied to it or reacted.
 */
export function isAwaitingReply(message: Message, threadRoot: string | null, me: string): boolean {
  if (threadRoot !== null) return false;
  const hasReplyFromOthers = message.recent_reply_authors.some((author) => author !== me);
  const hasReactionFromOthers = message.reactions.some((reaction) => reaction.count > (reaction.mine ? 1 : 0));
  if (hasReplyFromOthers || hasReactionFromOthers) return false;
  return message.text.includes("?") || MENTION_LINK.test(message.text);
}
