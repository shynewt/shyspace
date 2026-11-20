import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { api, type Message, type MessageCursor } from "../../lib/api";
import { newestMessageEndingInView } from "../../lib/messageList";
import { reportError } from "../../lib/toast";

/** Reading moves the position often; it's saved at most this long after it first moves. */
const SAVE_WITHIN_MS = 300;

/**
 * The positions this device last sent, by channel or thread. The core's
 * answer can lag a moment behind (a reload in flight), and a channel opened
 * again right away mustn't show a line for what was just read.
 */
const sentPositions = new Map<string, MessageCursor>();

export function cursorOf(message: Message): MessageCursor {
  return { lamport: message.lamport, author: message.author };
}

export function isAfter(message: MessageCursor, cursor: MessageCursor): boolean {
  return message.lamport > cursor.lamport || (message.lamport === cursor.lamport && message.author > cursor.author);
}

/** The newer of the core's read position for `key` and the one this device last sent. */
export function knownReadPosition(key: string, fromCore: MessageCursor | null): MessageCursor | null {
  const sent = sentPositions.get(key);
  if (!sent) return fromCore;
  return !fromCore || isAfter(sent, fromCore) ? sent : fromCore;
}

/**
 * Where you've read up to in one list (a channel, or a thread: `key` names
 * it). A message is read once its bottom edge has been on screen while you
 * could be looking (`isLooking`: the window has focus and is visible, and
 * this is the view you're in). The position only moves forward, and is saved
 * soon after it moves, when you leave, and when the window loses focus or
 * hides. If a save fails because its message was just deleted, the position
 * goes back to the last saved one and what's on screen is read again.
 */
export function useReadPosition({
  key,
  list,
  messages,
  initial,
  isLooking,
  save,
}: {
  key: string;
  list: RefObject<HTMLElement | null>;
  messages: Message[] | undefined;
  initial: MessageCursor | null;
  isLooking: boolean;
  save: (messageId: string) => Promise<void>;
}) {
  const [readCursor, setReadCursor] = useState(initial);
  const latest = useRef({ messages, readCursor, isLooking, save });
  latest.current = { messages, readCursor, isLooking, save };
  const saved = useRef(initial);
  const unsaved = useRef<Message | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const needsRereading = useRef(false);
  const markSeenRef = useRef<() => void>(() => {});

  const setCursor = useCallback((cursor: MessageCursor | null) => {
    latest.current.readCursor = cursor;
    setReadCursor(cursor);
  }, []);

  const flush = useCallback(() => {
    clearTimeout(timer.current);
    timer.current = undefined;
    const message = unsaved.current;
    if (!message) return;
    unsaved.current = null;
    const cursor = cursorOf(message);
    sentPositions.set(key, cursor);
    latest.current.save(message.id).then(
      () => {
        if (!saved.current || isAfter(cursor, saved.current)) saved.current = cursor;
      },
      (error) => {
        if (saved.current) sentPositions.set(key, saved.current);
        else sentPositions.delete(key);
        setCursor(saved.current);
        // A message deleted just after you read it fails the save harmlessly: read again once the list has caught up.
        api.linkedMessage(message.id).then(
          (linked) => {
            if (linked) return reportError(error);
            const isStillShown = latest.current.messages?.some((candidate) => candidate.id === message.id);
            if (isStillShown) needsRereading.current = true;
            else markSeenRef.current();
          },
          () => reportError(error),
        );
      },
    );
  }, [key, setCursor]);

  const moveTo = useCallback(
    (message: Message) => {
      const { readCursor: current } = latest.current;
      if (current && !isAfter(cursorOf(message), current)) return;
      setCursor(cursorOf(message));
      unsaved.current = message;
      timer.current ??= setTimeout(flush, SAVE_WITHIN_MS);
    },
    [flush, setCursor],
  );

  /** Marks read what's on screen now, if you could be looking. */
  const markSeen = useCallback(() => {
    const element = list.current;
    const { messages: shown, isLooking: looking } = latest.current;
    if (!element || !shown || !looking) return;
    const id = newestMessageEndingInView(element);
    const message = id ? shown.find((candidate) => candidate.id === id) : undefined;
    if (message) moveTo(message);
  }, [list, moveTo]);
  markSeenRef.current = markSeen;

  useEffect(() => {
    if (!messages || !needsRereading.current) return;
    needsRereading.current = false;
    markSeen();
  }, [messages, markSeen]);

  /** Everything is read, as Esc says; saved at once. */
  const markAllRead = useCallback(() => {
    const newest = latest.current.messages?.at(-1);
    if (!newest) return;
    const isSaved = saved.current !== null && !isAfter(cursorOf(newest), saved.current);
    if (isSaved && !unsaved.current) return;
    setCursor(cursorOf(newest));
    unsaved.current = newest;
    flush();
  }, [flush, setCursor]);

  // Coming back to the window: what's on screen becomes read.
  useEffect(() => {
    if (isLooking) markSeen();
    else flush();
  }, [isLooking, markSeen, flush]);

  useEffect(() => {
    window.addEventListener("pagehide", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      flush();
    };
  }, [flush]);

  return { readCursor, markSeen, markAllRead };
}
