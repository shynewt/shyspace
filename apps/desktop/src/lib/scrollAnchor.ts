/**
 * Where a message list is scrolled, as the top visible message and how far
 * its top sits from the list's top edge. Unlike a pixel offset, this stays
 * right when messages above load, change height or wrap differently, and it
 * doesn't rely on CSS `overflow-anchor`, which WebKit lacks.
 *
 * Messages are `article` elements with the id `message-<id>` inside a
 * positioned list. Positions come from layout offsets, not on-screen boxes,
 * so they ignore the transforms that animate rows in.
 */

export type ScrollAnchor = {
  messageId: string;
  /** From the list's top edge to the message's top; negative when it's partly scrolled past. */
  offsetPx: number;
};

/** A request to scroll a list back: to the anchor, or to its newest message when there's none. */
export type ScrollRestore = { anchor: ScrollAnchor | null };

const MESSAGE_ID_PREFIX = "message-";

/** Messages that are laid out; a hidden one has no position to measure. */
function messagesIn(list: HTMLElement): HTMLElement[] {
  return Array.from(list.querySelectorAll<HTMLElement>(`article[id^="${MESSAGE_ID_PREFIX}"]`)).filter(
    (message) => message.offsetParent !== null,
  );
}

/** From the top of the list's content to the element's top. A row being animated can be an offset parent of its own, so this adds up the chain. */
function topWithin(list: HTMLElement, element: HTMLElement): number {
  let top = 0;
  let current: Element | null = element;
  while (current instanceof HTMLElement && current !== list) {
    top += current.offsetTop;
    current = current.offsetParent;
  }
  if (current !== list) throw new Error("a message list must be positioned to measure where its messages are");
  return top;
}

/** The first message whose bottom is below the list's top edge, or null for an empty list. */
export function captureScrollAnchor(list: HTMLElement): ScrollAnchor | null {
  const top = list.scrollTop;
  for (const message of messagesIn(list)) {
    const messageTop = topWithin(list, message);
    if (messageTop + message.offsetHeight > top) {
      return { messageId: message.id.slice(MESSAGE_ID_PREFIX.length), offsetPx: messageTop - top };
    }
  }
  return null;
}

/** Scrolls so the anchor's message sits where it was. Returns false when that message isn't in the list. */
export function restoreScrollAnchor(list: HTMLElement, anchor: ScrollAnchor): boolean {
  const message = messagesIn(list).find((candidate) => candidate.id === `${MESSAGE_ID_PREFIX}${anchor.messageId}`);
  if (!message) return false;
  list.scrollTop = topWithin(list, message) - anchor.offsetPx;
  return true;
}
