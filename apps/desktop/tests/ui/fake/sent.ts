import type { LinkedMessage, MessageCursor } from "../../../src/lib/api";
import type { FakeChat } from "./chat";
import { matchesTerms, searchTerms } from "./search";

type SentQuery = {
  me: string;
  isReadable: (channel: string) => boolean;
  text: string;
  before: MessageCursor | null;
  limit: number;
};

/** Like the core: your own messages in channels you can still read, newest first, narrowed by words. */
export function sentMessages(chat: FakeChat, { me, isReadable, text, before, limit }: SentQuery): LinkedMessage[] {
  const terms = searchTerms(text);
  const isOlder = (message: { lamport: number; device: string }) =>
    !before ||
    message.lamport < before.lamport ||
    (message.lamport === before.lamport && message.device < before.device);
  return chat.messages
    .filter(
      (message) =>
        message.author === me && isReadable(message.channel) && isOlder(message) && matchesTerms(terms, message.text),
    )
    .sort((left, right) => right.lamport - left.lamport || right.device.localeCompare(left.device))
    .slice(0, limit)
    .flatMap((message) => chat.linkedMessage(message.id) ?? []);
}
