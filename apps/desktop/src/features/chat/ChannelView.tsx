import { AnimatePresence } from "motion/react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, type Message } from "../../lib/api";
import { channelReference, composerLabel } from "../../lib/channels";
import { useMembers } from "../../lib/members";
import { distanceFromBottom, isAboveView, isInView } from "../../lib/messageList";
import { captureScrollAnchor, restoreScrollAnchor, type ScrollRestore } from "../../lib/scrollAnchor";
import { useLive } from "../../lib/useLive";
import { useScrollMemory } from "../../lib/useNavigation";
import { useWindowAttention } from "../../lib/useWindowAttention";
import { useWorkspace } from "../../lib/workspace";
import { ChannelTitle } from "../private/ChannelTitle";
import { WriteGate } from "../private/WriteGate";
import { ViewHeader } from "../shell/ViewHeader";
import { ChannelInfo } from "./ChannelInfo";
import { Composer } from "./Composer";
import { firstUnreadId, MessageRows, toRows, UNREAD_LINE_ID } from "./channelRows";
import { LocalTimeHint } from "./LocalTimeHint";
import { ThreadPanel } from "./ThreadPanel";
import { UnreadPills } from "./UnreadPills";
import { NEAR_EDGE_PX, useHeldScroll } from "./useHeldScroll";
import { isAfter, knownReadPosition, useReadPosition } from "./useReadPosition";
import type { InsertMention } from "./useSuggestions";

const PAGE_SIZE = 100;
const HIGHLIGHT_MS = 1600;
/** How much older history to load looking for where you were, before opening as usual. */
const MAX_PAGES_FOR_ANCHOR = 10;

/** "pending": not decided yet for this visit; "done": hidden for the rest of it. */
type JumpPill = "pending" | "shown" | "done";

export function ChannelView({
  channelId,
  thread,
  focus,
  restore,
}: {
  channelId: string;
  /** The open thread's first message. */
  thread?: string;
  /** A message to scroll to and highlight. */
  focus?: string;
  /** Where to scroll to, coming back here; each new request is carried out once. */
  restore: ScrollRestore | null;
}) {
  const { channels, session, navigate } = useWorkspace();
  const { registerAnchorSource, noteScrolled } = useScrollMemory();
  const channel = channels.find((candidate) => candidate.id === channelId);
  const members = useMembers();
  const isLooking = useWindowAttention();
  // Loading older messages grows the window, so new arrivals never leave gaps.
  const [windowSize, setWindowSize] = useState(PAGE_SIZE);
  const loadMessages = useCallback(() => api.messages(channelId, null, windowSize), [channelId, windowSize]);
  const messages = useLive(
    loadMessages,
    (change) => (change.kind === "messages" && change.channel === channelId) || change.kind === "members",
  );
  // The messages shown when more history was asked for, until the bigger load replaces them.
  const [messagesBeforeGrowing, setMessagesBeforeGrowing] = useState<Message[] | null>(null);
  const isGrowing = messages !== undefined && messages === messagesBeforeGrowing;
  const hasOlder = isGrowing || (messages?.length ?? 0) >= windowSize;
  const growWindow = useCallback(() => {
    setMessagesBeforeGrowing(messages ?? null);
    setWindowSize((size) => size + PAGE_SIZE);
  }, [messages]);
  const openThread = thread ?? null;
  const mentionTarget = useRef<InsertMention | null>(null);
  const mention = useCallback((name: string, memberId: string) => mentionTarget.current?.(name, memberId), []);
  const [highlighted, setHighlighted] = useState<string | null>(null);
  const [editing, setEditing] = useState<Message | null>(null);
  // A linked message to show: in the channel (loading older history until
  // it's there), or in the open thread, which handles it.
  const [pendingFocus, setPendingFocus] = useState(thread ? null : (focus ?? null));
  useEffect(() => {
    if (focus && !thread) setPendingFocus(focus);
  }, [focus, thread]);
  const [pendingRestore, setPendingRestore] = useState(restore);
  const pagesLoadedForAnchor = useRef(0);
  useEffect(() => {
    if (!restore) return;
    pagesLoadedForAnchor.current = 0;
    setPendingRestore(restore);
  }, [restore]);

  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const [lastReadOnOpen] = useState(() => knownReadPosition(channelId, channel?.last_read ?? null));
  const read = useReadPosition({
    key: channelId,
    list: scroller,
    messages,
    initial: lastReadOnOpen,
    isLooking,
    save: (messageId) => api.markRead(channelId, messageId),
  });
  const { markSeen, markAllRead, readCursor } = read;
  // The line sits above the first message that was unread when you opened the channel, and stays put while you read.
  // Null until the first messages load; `above` is null when nothing was unread.
  const [line, setLine] = useState<{ above: string | null } | null>(null);
  if (messages && line === null) setLine({ above: firstUnreadId(messages, lastReadOnOpen, session.me) });
  const [isLineDismissed, setLineDismissed] = useState(false);
  const unreadFrom = isLineDismissed || !line ? null : line.above;
  const rows = useMemo(() => toRows(messages ?? [], unreadFrom), [messages, unreadFrom]);
  const [jumpPill, setJumpPill] = useState<JumpPill>("pending");
  const [newCount, setNewCount] = useState(0);

  const view = useHeldScroll(scroller, markSeen);
  const { isFollowing, hold, follow, decideFollowing, noteScroll } = view;
  const hasScrolledToUnread = useRef(false);
  const newestShown = useRef<string | undefined>(undefined);
  const hasPositioned = useRef(false);

  // Messages that arrive while you're away from the newest one are counted, and kept in the loaded window
  // so the message you're reading isn't pushed out of it.
  const countArrivals = useCallback(
    (shown: Message[]) => {
      const previous = newestShown.current;
      newestShown.current = shown.at(-1)?.id;
      if (!previous || isFollowing.current) return;
      const index = shown.findIndex((message) => message.id === previous);
      if (index < 0) return;
      const arrivals = shown.slice(index + 1);
      const fromOthers = arrivals.filter((message) => message.author !== session.me).length;
      if (fromOthers > 0) setNewCount((count) => count + fromOthers);
      if (arrivals.length > 0) setWindowSize((size) => size + arrivals.length);
    },
    [session.me, isFollowing],
  );

  /** Decided once per visit, right after the view first settles: a way up if the first unread is above. */
  const decideJumpPill = useCallback(() => {
    const element = scroller.current;
    const unreadLine = document.getElementById(UNREAD_LINE_ID);
    setJumpPill((current) =>
      current !== "pending" ? current : element && unreadLine && isAboveView(element, unreadLine) ? "shown" : "done",
    );
  }, []);

  // Positions the view whenever the rows change. A message link to show waits for its own effect.
  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element || !messages || rows.length === 0 || isGrowing) return;
    countArrivals(messages);
    if (pendingFocus) return;
    const wasPositioned = hasPositioned.current;
    hasPositioned.current = true;
    if (pendingRestore) {
      const { anchor } = pendingRestore;
      if (!anchor) {
        // Back to the newest message; a channel just opened goes on as usual instead (the unread line, or the newest).
        setPendingRestore(null);
        if (!wasPositioned) return;
        follow();
      } else if (restoreScrollAnchor(element, anchor)) {
        hasScrolledToUnread.current = true;
        setPendingRestore(null);
        decideFollowing();
      } else if (hasOlder && pagesLoadedForAnchor.current < MAX_PAGES_FOR_ANCHOR) {
        // Older history loads until the anchor's message is there; a deleted one stops the search eventually.
        pagesLoadedForAnchor.current += 1;
        growWindow();
        return;
      } else {
        setPendingRestore(null);
        return;
      }
    } else if (!hasScrolledToUnread.current && unreadFrom) {
      hasScrolledToUnread.current = true;
      document.getElementById(UNREAD_LINE_ID)?.scrollIntoView({ block: "start" });
      decideFollowing();
    } else {
      // Only opening goes to the line; a line added later (for messages that came while you were away) doesn't move you.
      hasScrolledToUnread.current = true;
      hold();
    }
    if (distanceFromBottom(element) <= 1) {
      isFollowing.current = true;
      setNewCount(0);
    }
    decideJumpPill();
  }, [
    messages,
    rows,
    unreadFrom,
    pendingRestore,
    pendingFocus,
    hasOlder,
    isGrowing,
    growWindow,
    countArrivals,
    hold,
    follow,
    decideFollowing,
    decideJumpPill,
    isFollowing,
  ]);

  // Content that changes size on its own (cards loading, code folding, the window resizing) mustn't move the view.
  useEffect(() => {
    const element = scroller.current;
    const inner = content.current;
    if (!element || !inner) return;
    const observer = new ResizeObserver(() => {
      if (hasPositioned.current) hold();
    });
    observer.observe(element);
    observer.observe(inner);
    return () => observer.disconnect();
  }, [hold]);

  // Messages from others arriving while you can't be looking are unread; the line goes above the first of them,
  // unless one already marks unread messages.
  useEffect(() => {
    if (isLooking || !messages) return;
    const lineMessage = unreadFrom ? messages.find((message) => message.id === unreadFrom) : undefined;
    if (lineMessage && (!readCursor || isAfter(lineMessage, readCursor))) return;
    const firstUnseen = firstUnreadId(messages, readCursor, session.me);
    if (firstUnseen === null || firstUnseen === unreadFrom) return;
    setLine({ above: firstUnseen });
    setLineDismissed(false);
  }, [messages, isLooking, unreadFrom, readCursor, session.me]);

  // Where you are, for coming back here: nothing while at the newest message, which is where a channel opens anyway.
  const pendingAnchorRef = useRef(pendingRestore?.anchor);
  pendingAnchorRef.current = pendingRestore?.anchor;
  useEffect(
    () =>
      registerAnchorSource(channelId, () => {
        const element = scroller.current;
        if (pendingAnchorRef.current) return pendingAnchorRef.current;
        if (!element || isFollowing.current) return null;
        return captureScrollAnchor(element);
      }),
    [channelId, registerAnchorSource, isFollowing],
  );

  const jumpToNewest = useCallback(() => {
    setNewCount(0);
    follow();
  }, [follow]);

  useEffect(() => {
    // Escape that closed something else, like a menu, the palette or a name field, isn't for the channel.
    const markAllReadOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      markAllRead();
      setLineDismissed(true);
      setJumpPill("done");
      jumpToNewest();
    };
    window.addEventListener("keydown", markAllReadOnEscape);
    return () => window.removeEventListener("keydown", markAllReadOnEscape);
  }, [markAllRead, jumpToNewest]);

  useEffect(() => {
    if (!pendingFocus || !messages || isGrowing) return;
    const target = pendingFocus;
    if (messages.some((message) => message.id === target)) {
      setPendingFocus(null);
      hasPositioned.current = true;
      hasScrolledToUnread.current = true;
      document.getElementById(`message-${target}`)?.scrollIntoView({ block: "center" });
      decideFollowing();
      decideJumpPill();
      setHighlighted(target);
    } else if (hasOlder) {
      growWindow();
    } else {
      setPendingFocus(null);
    }
  }, [messages, hasOlder, isGrowing, pendingFocus, growWindow, decideFollowing, decideJumpPill]);

  useEffect(() => {
    if (!highlighted) return;
    const timer = setTimeout(() => setHighlighted(null), HIGHLIGHT_MS);
    return () => clearTimeout(timer);
  }, [highlighted]);

  const loadOlder = () => {
    if (hasOlder && !isGrowing && !pendingRestore) growWindow();
  };

  // A click away closes a thread over the channel on pointer-down; the click that follows, on the line or
  // button that opens that same thread, mustn't reopen it. Forgotten at the next click or pointer-down.
  const threadClosedByClickAway = useRef<string | null>(null);
  const closeThreadByClickAway = () => {
    threadClosedByClickAway.current = openThread;
    const forget = () => {
      threadClosedByClickAway.current = null;
      window.removeEventListener("click", forget);
      window.removeEventListener("pointerdown", forget, { capture: true });
    };
    // On the window, so it runs after React's handlers for this click; capture, so not for this pointer-down.
    window.addEventListener("click", forget);
    window.addEventListener("pointerdown", forget, { capture: true });
    navigate({ kind: "channel", id: channelId });
  };
  const openThreadOf = (message: Message) => {
    if (threadClosedByClickAway.current === message.id) return;
    navigate({ kind: "channel", id: channelId, thread: message.id });
  };

  /** After a pill, keyboard focus goes to the messages rather than being lost with the pill. */
  const focusMessages = () => scroller.current?.focus({ preventScroll: true });

  const jumpToUnread = () => {
    setJumpPill("done");
    document.getElementById(UNREAD_LINE_ID)?.scrollIntoView({ block: "start" });
    decideFollowing();
    focusMessages();
  };

  const onScroll = (element: HTMLElement) => {
    if (noteScroll()) setNewCount(0);
    if (jumpPill === "shown") {
      const unreadLine = document.getElementById(UNREAD_LINE_ID);
      if (!unreadLine || isInView(element, unreadLine)) setJumpPill("done");
    }
    if (element.scrollTop < NEAR_EDGE_PX) loadOlder();
    noteScrolled();
  };

  return (
    <div className="relative flex h-full">
      <div className="flex min-w-0 flex-1 flex-col">
        <ViewHeader>
          {channel ? <ChannelTitle channel={channel} members={members} /> : <h1 className="text-lg">…</h1>}
          <ChannelInfo />
        </ViewHeader>

        <div className="relative flex min-h-0 flex-1 flex-col">
          <div
            ref={scroller}
            role="log"
            aria-label={`Messages in ${channelReference(channel, members)}`}
            aria-busy={messages === undefined}
            tabIndex={-1}
            onScroll={(event) => onScroll(event.currentTarget)}
            className="relative flex-1 overflow-y-auto py-3 outline-none [overflow-anchor:none]"
          >
            <div ref={content}>
              <MessageRows
                channel={channel}
                members={members}
                messages={messages}
                rows={rows}
                hasOlder={hasOlder}
                isHighlighted={(message) =>
                  openThread === message.id || highlighted === message.id || editing?.id === message.id
                }
                onOpenThread={openThreadOf}
                onEdit={setEditing}
                onMention={mention}
              />
            </div>
          </div>
          <UnreadPills
            isJumpToUnreadShown={jumpPill === "shown"}
            newCount={newCount}
            onJumpToUnread={jumpToUnread}
            onJumpToNewest={() => {
              jumpToNewest();
              focusMessages();
            }}
          />
        </div>

        <WriteGate channelId={channelId}>
          <LocalTimeHint channel={channel} />
          <Composer
            channelId={channelId}
            label={composerLabel(channel, members)}
            threadRoot={null}
            onSent={jumpToNewest}
            editing={editing}
            onEditDone={() => setEditing(null)}
            onEditLast={() => setEditing(messages?.findLast((message) => message.can_edit) ?? null)}
            mentionTarget={mentionTarget}
          />
        </WriteGate>
      </div>
      <AnimatePresence>
        {openThread && (
          <ThreadPanel
            key={openThread}
            root={openThread}
            focus={focus}
            channelId={channelId}
            onClose={() => navigate({ kind: "channel", id: channelId })}
            onClickAway={closeThreadByClickAway}
            returnFocusTo={scroller}
          />
        )}
      </AnimatePresence>
    </div>
  );
}
