import { type RefObject, useCallback, useRef } from "react";
import { distanceFromBottom, messageTop } from "../../lib/messageList";
import { captureScrollAnchor } from "../../lib/scrollAnchor";

/** Content arriving within this distance of the newest message keeps you following it. */
export const NEAR_EDGE_PX = 80;
/** Scrolling back down this close to the newest message follows it again. */
const REPIN_PX = 24;

/**
 * Keeps a message list where you want it as its content changes: following
 * the newest message while you're there, otherwise holding the top message
 * exactly in place. Scrolling up by any means stops following at once;
 * scrolling back down within 24 px resumes it. Moves only by the difference,
 * so a fling in progress carries on.
 */
export function useHeldScroll(list: RefObject<HTMLElement | null>, onSettled: () => void) {
  const isFollowing = useRef(true);
  /** The top message as of the last settle, and where its top was. */
  const held = useRef<{ messageId: string; top: number } | null>(null);
  const lastScrollTop = useRef(0);

  /** Records where things are now, after the view moved or was moved. */
  const settle = useCallback(() => {
    const element = list.current;
    if (!element) return;
    const anchor = captureScrollAnchor(element);
    held.current = anchor && { messageId: anchor.messageId, top: anchor.offsetPx + element.scrollTop };
    lastScrollTop.current = element.scrollTop;
    onSettled();
  }, [list, onSettled]);

  /** Puts the view back where it belongs after content changed, then settles. */
  const hold = useCallback(() => {
    const element = list.current;
    if (!element) return;
    const anchor = held.current;
    if (isFollowing.current) {
      if (distanceFromBottom(element) > 0) element.scrollTop = element.scrollHeight;
    } else if (anchor) {
      const top = messageTop(element, anchor.messageId);
      if (top !== null && top !== anchor.top) element.scrollTop += top - anchor.top;
    }
    settle();
  }, [list, settle]);

  /** Follows from now on, scrolled to the newest message. */
  const follow = useCallback(() => {
    const element = list.current;
    isFollowing.current = true;
    if (element) element.scrollTop = element.scrollHeight;
    settle();
  }, [list, settle]);

  /** After the view was moved on purpose (to a message, the line, a saved place): follow only if that's the bottom. */
  const decideFollowing = useCallback(() => {
    const element = list.current;
    if (element) isFollowing.current = distanceFromBottom(element) <= REPIN_PX;
    settle();
  }, [list, settle]);

  /** Updates following from a scroll event and settles; returns whether it's following now. */
  const noteScroll = useCallback((): boolean => {
    const element = list.current;
    if (!element) return isFollowing.current;
    const distance = distanceFromBottom(element);
    const isScrollingUp = element.scrollTop < lastScrollTop.current;
    if (distance <= REPIN_PX && !isScrollingUp) isFollowing.current = true;
    else if (distance > NEAR_EDGE_PX || (isScrollingUp && distance > 1)) isFollowing.current = false;
    settle();
    return isFollowing.current;
  }, [list, settle]);

  return { isFollowing, hold, follow, decideFollowing, noteScroll, settle };
}
