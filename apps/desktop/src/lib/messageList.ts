/**
 * Where things are in a scrolling message list, by layout (see
 * `scrollAnchor.ts` for the list's requirements).
 */
import { MESSAGE_ID_PREFIX, messagesIn, topWithin } from "./scrollAnchor";

export function distanceFromBottom(list: HTMLElement): number {
  return list.scrollHeight - list.scrollTop - list.clientHeight;
}

/** The newest message whose bottom edge is on screen, or null when none is. */
export function newestMessageEndingInView(list: HTMLElement): string | null {
  const top = list.scrollTop;
  const bottom = top + list.clientHeight;
  let newest: string | null = null;
  for (const message of messagesIn(list)) {
    const end = topWithin(list, message) + message.offsetHeight;
    if (end > bottom) break;
    if (end >= top) newest = message.id.slice(MESSAGE_ID_PREFIX.length);
  }
  return newest;
}

/** Whether `element`, inside the list, is entirely above what's on screen. */
export function isAboveView(list: HTMLElement, element: HTMLElement): boolean {
  return topWithin(list, element) + element.offsetHeight <= list.scrollTop;
}

/** Whether any part of `element`, inside the list, is on screen. */
export function isInView(list: HTMLElement, element: HTMLElement): boolean {
  const top = topWithin(list, element);
  return top < list.scrollTop + list.clientHeight && top + element.offsetHeight > list.scrollTop;
}

/** The layout top of message `id` in the list, or null when it isn't there. */
export function messageTop(list: HTMLElement, id: string): number | null {
  const message = messagesIn(list).find((candidate) => candidate.id === `${MESSAGE_ID_PREFIX}${id}`);
  return message ? topWithin(list, message) : null;
}
