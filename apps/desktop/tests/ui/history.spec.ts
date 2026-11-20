/** Back and forward through the places you've been. Runs without a browser. */
import { expect, test } from "@playwright/test";
import {
  currentEntry,
  type History,
  leaveWithAnchor,
  MAX_HISTORY_ENTRIES,
  replaceCurrent,
  startHistory,
  step,
  visit,
} from "../../src/lib/history";
import type { View } from "../../src/lib/workspace";

const channel = (id: string, thread?: string): View => ({ kind: "channel", id, thread });
const everywhere = () => true;

function viewsOf(history: History): View[] {
  return history.entries.map((entry) => entry.view);
}

test("back returns to the previous place and forward comes back", () => {
  let history = visit(startHistory(channel("a")), channel("b"));
  const back = step(history, -1, everywhere);
  expect(back.outcome).toBe("moved");
  expect(currentEntry(back.history).view).toEqual(channel("a"));

  history = step(back.history, 1, everywhere).history;
  expect(currentEntry(history).view).toEqual(channel("b"));
});

test("a new place after going back clears what was ahead", () => {
  let history = visit(visit(startHistory(channel("a")), channel("b")), channel("c"));
  history = step(history, -1, everywhere).history;
  history = visit(history, { kind: "inbox" });

  expect(viewsOf(history)).toEqual([channel("a"), channel("b"), { kind: "inbox" }]);
  expect(step(history, 1, everywhere).outcome).toBe("none");
});

test("opening and closing a thread are places of their own", () => {
  let history = visit(startHistory(channel("a")), channel("a", "root"));
  history = visit(history, channel("a"));

  expect(viewsOf(history)).toEqual([channel("a"), channel("a", "root"), channel("a")]);
});

test("opening the place you're already in adds nothing", () => {
  const history = visit(startHistory(channel("a", "root")), channel("a", "root"));

  expect(history.entries).toHaveLength(1);
});

test("a jump to a message is a place even in the same channel", () => {
  const history = visit(startHistory(channel("a")), { kind: "channel", id: "a", focus: "m1" });

  expect(history.entries).toHaveLength(2);
});

test("history keeps the most recent places only", () => {
  let history = startHistory({ kind: "page", id: "0" });
  for (let index = 1; index < MAX_HISTORY_ENTRIES + 10; index += 1) {
    history = visit(history, { kind: "page", id: String(index) });
  }

  expect(history.entries).toHaveLength(MAX_HISTORY_ENTRIES);
  expect(history.index).toBe(MAX_HISTORY_ENTRIES - 1);
  expect(history.entries[0].view).toEqual({ kind: "page", id: "10" });
});

test("leaving a place remembers where it was scrolled, not the message it jumped to", () => {
  let history = startHistory({ kind: "channel", id: "a", focus: "m1" });
  history = leaveWithAnchor(history, { messageId: "m7", offsetPx: -12 });
  history = visit(history, channel("b"));

  const back = step(history, -1, everywhere).history;
  expect(currentEntry(back)).toEqual({ view: channel("a"), anchor: { messageId: "m7", offsetPx: -12 } });
});

test("a new place can start at a remembered position", () => {
  const anchor = { messageId: "m3", offsetPx: 4 };
  const history = visit(startHistory(channel("a")), channel("b"), anchor);

  expect(currentEntry(history).anchor).toEqual(anchor);
});

test("going back skips places that are gone and forgets them", () => {
  const gone = new Set(["b", "c"]);
  const isAvailable = (view: View) => !(view.kind === "channel" && gone.has(view.id));
  let history = startHistory(channel("a"));
  for (const id of ["b", "c", "d"]) history = visit(history, channel(id));

  const back = step(history, -1, isAvailable);
  expect(back.outcome).toBe("moved");
  expect(currentEntry(back.history).view).toEqual(channel("a"));
  expect(viewsOf(back.history)).toEqual([channel("a"), channel("d")]);
});

test("going back says so when every earlier place is gone", () => {
  const isAvailable = (view: View) => !(view.kind === "channel" && view.id === "a");
  const history = visit(startHistory(channel("a")), channel("b"));

  const back = step(history, -1, isAvailable);
  expect(back.outcome).toBe("all-skipped");
  expect(viewsOf(back.history)).toEqual([channel("b")]);
  expect(back.history.index).toBe(0);
});

test("going back with nowhere to go changes nothing", () => {
  const history = startHistory(channel("a"));

  expect(step(history, -1, everywhere)).toEqual({ history, outcome: "none" });
});

test("replacing the current place keeps the rest", () => {
  let history = visit(startHistory(channel("a")), channel("b", "root"));
  history = replaceCurrent(history, channel("b"));

  expect(viewsOf(history)).toEqual([channel("a"), channel("b")]);
  expect(history.index).toBe(1);
});
