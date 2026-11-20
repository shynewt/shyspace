import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, type Channel, type Page } from "./api";
import {
  currentEntry,
  type History,
  type HistoryEntry,
  leaveWithAnchor,
  replaceCurrent,
  startHistory,
  step,
  visit,
} from "./history";
import { readLastView, saveLastView } from "./lastView";
import type { ScrollAnchor, ScrollRestore } from "./scrollAnchor";
import { reportError, showToast } from "./toast";
import type { View } from "./workspace";

/** Reads where a channel's messages are scrolled right now; null when at the newest message. */
type AnchorSource = { channelId: string; capture: () => ScrollAnchor | null };

type ScrollMemory = {
  /** A shown channel says how to read its scroll position; returns the function that stops it. */
  registerAnchorSource: (channelId: string, capture: () => ScrollAnchor | null) => () => void;
  /** The shown channel scrolled: remember the position soon. */
  noteScrolled: () => void;
};

export const ScrollMemoryContext = createContext<ScrollMemory | null>(null);

export function useScrollMemory(): ScrollMemory {
  const value = useContext(ScrollMemoryContext);
  if (!value) throw new Error("useScrollMemory must be used inside the workspace shell");
  return value;
}

const SAVE_AFTER_SCROLL_MS = 500;

/**
 * Where you are and where you've been: back and forward, each place opening
 * where you left it, and the app reopening at the last place on launch.
 * `channels` and `pages` are undefined until loaded.
 */
export function useNavigation(me: string, channels: Channel[] | undefined, pages: Page[] | undefined) {
  const [shown, setShown] = useState<{ history: History; restore: ScrollRestore | null } | null>(null);
  const history = shown?.history ?? null;
  // Read and written synchronously, so quick presses of Back build on each other.
  const historyRef = useRef<History | null>(null);
  const anchorSource = useRef<AnchorSource | null>(null);
  const rememberedAnchors = useRef(new Map<string, ScrollAnchor>());
  const threadCheck = useRef(0);
  const latest = useRef({ channels, pages });
  latest.current = { channels, pages };

  /** Shows `next`; `restore` says where to scroll its channel, if anywhere. */
  const commit = useCallback((next: History | null, restore: ScrollRestore | null = null) => {
    threadCheck.current += 1;
    historyRef.current = next;
    setShown(next ? { history: next, restore } : null);
  }, []);

  /** Where the shown channel is scrolled, if it's the current place's. */
  const captureCurrent = useCallback((): ScrollAnchor | null => {
    const current = historyRef.current;
    const source = anchorSource.current;
    if (!current || !source) return null;
    const { view, anchor } = currentEntry(current);
    if (view.kind !== "channel" || view.id !== source.channelId) return anchor;
    const captured = source.capture();
    if (captured) rememberedAnchors.current.set(view.id, captured);
    else rememberedAnchors.current.delete(view.id);
    return captured;
  }, []);

  /** Shows a place reached through history, checking first that its thread still exists. */
  const showChecked = useCallback(
    (next: History) => {
      const { view, anchor } = currentEntry(next);
      const restore = { anchor };
      if (view.kind !== "channel" || !view.thread) return commit(next, restore);
      const root = view.thread;
      historyRef.current = next;
      const check = ++threadCheck.current;
      const showChannelOnly = () => {
        const shown = historyRef.current ?? next;
        commit(replaceCurrent(shown, { kind: "channel", id: view.id }, anchor), restore);
      };
      api.linkedMessage(root).then(
        (linked) => {
          if (check !== threadCheck.current) return;
          if (linked && linked.message.channel === view.id) commit(historyRef.current, restore);
          else showChannelOnly();
        },
        (error) => {
          if (check !== threadCheck.current) return;
          reportError(error);
          showChannelOnly();
        },
      );
    },
    [commit],
  );

  const isAvailable = useCallback((view: View) => {
    const { channels: loadedChannels = [], pages: loadedPages = [] } = latest.current;
    // A private channel you were removed from stays listed, read-only, but is no longer a place to go back to.
    if (view.kind === "channel") {
      return loadedChannels.some(
        (channel) => channel.id === view.id && (channel.kind !== "private" || channel.is_member),
      );
    }
    if (view.kind === "page") return loadedPages.some((page) => page.id === view.id);
    return true;
  }, []);

  /** A different channel without unread messages opens where you last left it. */
  const startingAnchor = useCallback((from: View, to: View): ScrollAnchor | null => {
    if (to.kind !== "channel" || to.focus || (from.kind === "channel" && from.id === to.id)) return null;
    const channel = latest.current.channels?.find((candidate) => candidate.id === to.id);
    if (!channel || channel.unread > 0) return null;
    return rememberedAnchors.current.get(to.id) ?? null;
  }, []);

  const navigate = useCallback(
    (view: View) => {
      const current = historyRef.current;
      if (!current) return commit(startHistory(view));
      const left = leaveWithAnchor(current, captureCurrent());
      const start = startingAnchor(currentEntry(current).view, view);
      commit(visit(left, view, start), start ? { anchor: start } : null);
    },
    [commit, captureCurrent, startingAnchor],
  );

  const move = useCallback(
    (direction: -1 | 1) => {
      const current = historyRef.current;
      if (!current) return;
      const { history: moved, outcome } = step(leaveWithAnchor(current, captureCurrent()), direction, isAvailable);
      if (outcome === "all-skipped") {
        showToast(
          direction === -1
            ? "The places before this were deleted or you can no longer open them."
            : "The places after this were deleted or you can no longer open them.",
          "info",
        );
      }
      if (outcome === "moved") showChecked(moved);
      else commit(moved);
    },
    [commit, captureCurrent, isAvailable, showChecked],
  );
  const back = useCallback(() => move(-1), [move]);
  const forward = useCallback(() => move(1), [move]);

  const replace = useCallback(
    (view: View | null) => {
      const current = historyRef.current;
      commit(view === null ? null : current ? replaceCurrent(current, view) : startHistory(view));
    },
    [commit],
  );

  // On launch, the last place if it still exists, or the first channel.
  useEffect(() => {
    if (historyRef.current || !channels || !pages) return;
    const last = readLastView(me);
    if (last && isAvailable(last.view)) showChecked(startHistory(last.view, last.anchor));
    else if (channels.length > 0) commit(startHistory({ kind: "channel", id: channels[0].id }));
  }, [me, channels, pages, isAvailable, showChecked, commit]);

  const save = useCallback(() => {
    const current = historyRef.current;
    if (current) saveLastView(me, { view: currentEntry(current).view, anchor: captureCurrent() });
  }, [me, captureCurrent]);

  useEffect(() => {
    if (history) save();
  }, [history, save]);

  const saveTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => {
    window.addEventListener("blur", save);
    return () => {
      window.removeEventListener("blur", save);
      clearTimeout(saveTimer.current);
    };
  }, [save]);

  const scrollMemory = useMemo<ScrollMemory>(
    () => ({
      registerAnchorSource: (channelId, capture) => {
        const source = { channelId, capture };
        anchorSource.current = source;
        return () => {
          if (anchorSource.current === source) anchorSource.current = null;
        };
      },
      noteScrolled: () => {
        clearTimeout(saveTimer.current);
        saveTimer.current = setTimeout(save, SAVE_AFTER_SCROLL_MS);
      },
    }),
    [save],
  );

  const entry: HistoryEntry | null = history ? currentEntry(history) : null;
  return { entry, restore: shown?.restore ?? null, navigate, back, forward, replace, scrollMemory };
}
