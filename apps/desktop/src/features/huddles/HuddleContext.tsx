import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type Change,
  type HuddlePeer,
  type IncomingHuddle,
  type MyHuddle,
  onAnyWorkspaceChange,
} from "../../lib/api";
import { channelReference } from "../../lib/channels";
import { displayName } from "../../lib/people";
import { usePreferences } from "../../lib/preferences";
import { SHORTCUTS } from "../../lib/shortcuts";
import { reportError, showToast, showUndoToast } from "../../lib/toast";
import { useLiveEverywhere } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { useWorkspaces } from "../shell/Workspaces";
import { huddleNews } from "./huddleText";

/** A link that comes back within this long shows nothing, so a blip never alarms anyone. */
export const RECONNECT_GRACE_MS = 3000;
/** A device that recovers within this long shows nothing: a chosen one unplugged hands over to the default within a second. */
const DEVICE_GRACE_MS = 2000;

type HuddleValue = {
  /** Your huddle, in whichever workspace holds it; null when you're in none, undefined until loaded. */
  mine: MyHuddle | null | undefined;
  /** Your huddle is in another workspace than the one shown, so its people and place aren't in this one's lists. */
  isElsewhere: boolean;
  /** The huddle ringing for you right now, if any. */
  incoming: IncomingHuddle | null;
  /** Who's talking right now, you included. */
  speaking: ReadonlySet<string>;
  /** Peers whose audio has been reconnecting for longer than the grace. */
  reconnecting: ReadonlySet<string>;
  /** Your microphone, or your speakers, have been failing for longer than the grace; the core keeps retrying. */
  isMicDown: boolean;
  isSpeakerDown: boolean;
  join: (channel: string) => void;
  leave: () => void;
  /** Joins the ringing huddle, showing its workspace first if it's another. */
  answer: () => void;
  /** Stops the ring without joining. */
  decline: () => void;
  toggleMute: () => void;
};

const HuddleContext = createContext<HuddleValue | null>(null);

export function useMyHuddle(): HuddleValue {
  const value = useContext(HuddleContext);
  if (!value) throw new Error("useMyHuddle needs a HuddleProvider");
  return value;
}

/**
 * Your huddle for the whole app: what it is, who's talking, the actions on
 * it, and what a screen reader hears as it changes.
 */
/** A huddle to join once its workspace is shown, after answering a ring from another. */
let pendingJoin: string | null = null;

export function HuddleProvider({ children }: { children: ReactNode }) {
  // Only one workspace on the device holds a call, and it goes on whichever is shown.
  const mine = useLiveEverywhere(api.myHuddle, (change) => change.kind === "my_huddle");
  const spaces = useWorkspaces();
  const { session, channels, members } = useWorkspace();
  const [{ joinsHuddlesMuted }] = usePreferences();
  const speaking = useSpeaking(mine ?? null, session.me);
  const reconnecting = useLongReconnects(mine?.peers ?? NO_PEERS);

  const callPlace = mine?.channel_name;
  const placeOf = useCallback(
    (channel: string) =>
      channelReference(
        channels.find((candidate) => candidate.id === channel),
        members,
      ) ||
      callPlace ||
      "a channel",
    [channels, members, callPlace],
  );
  const isElsewhere = Boolean(mine && mine.workspace_id !== session.workspace_id);
  const nameOf = useCallback(
    (member: string) => displayName(members?.find((candidate) => candidate.id === member)?.name ?? "", member),
    [members],
  );
  const [news, setNews] = useState<{ id: number; text: string } | null>(null);
  const say = useCallback((text: string) => setNews((last) => ({ id: (last?.id ?? 0) + 1, text })), []);
  useNews(mine, nameOf, placeOf, say);
  const incoming = useLiveEverywhere(api.huddleRinging, isRingChange) ?? null;
  const caller = incoming && displayName(incoming.member_name, incoming.member);
  useEffect(() => {
    if (caller) say(`${caller} started a huddle`);
  }, [caller, say]);
  const isMicDown = useLasting(mine?.mic === "lost", DEVICE_GRACE_MS);
  const isSpeakerDown = useLasting(mine?.output === "lost", DEVICE_GRACE_MS);
  useEffect(() => {
    if (isMicDown) say("Your microphone isn't working. Trying again");
  }, [isMicDown, say]);
  useEffect(() => {
    if (isSpeakerDown) say("Your speakers aren't working. Trying again");
  }, [isSpeakerDown, say]);
  const peerCount = mine?.peers.length ?? 0;
  const isReconnecting = peerCount > 0 && mine?.peers.every((peer) => reconnecting.has(peer.member)) === true;
  const wasReconnecting = useRef(false);
  useEffect(() => {
    if (isReconnecting === wasReconnecting.current) return;
    wasReconnecting.current = isReconnecting;
    // Leaving, or the others leaving, also ends a reconnect, but nothing reconnected then.
    if (isReconnecting) say("Reconnecting");
    else if (peerCount > 0) say("Reconnected");
  }, [isReconnecting, peerCount, say]);

  // Read when acting, so the actions stay stable while the huddle changes.
  const current = useRef(mine);
  current.current = mine;
  const join = useCallback(
    (channel: string) => {
      const previous = current.current;
      api.joinHuddle(channel, joinsHuddlesMuted).then(() => {
        if (!previous || previous.channel === channel) return;
        // Undo rejoins in the shown workspace, so only a huddle that was in it can come back that way.
        if (previous.workspace_id !== session.workspace_id) {
          return showToast(`Left the huddle in ${previous.channel_name}`, "info");
        }
        showUndoToast(`Left the huddle in ${placeOf(previous.channel)}`, () =>
          api.joinHuddle(previous.channel, joinsHuddlesMuted),
        );
      }, reportError);
    },
    [joinsHuddlesMuted, placeOf, session.workspace_id],
  );
  // Answering a ring from another workspace showed this one; join now.
  useEffect(() => {
    if (pendingJoin === null) return;
    const channel = pendingJoin;
    pendingJoin = null;
    join(channel);
  }, [join]);
  const ringing = useRef(incoming);
  ringing.current = incoming;
  const { select } = spaces;
  const answer = useCallback(() => {
    const ring = ringing.current;
    if (!ring) return;
    if (ring.workspace_id === session.workspace_id) return join(ring.channel);
    pendingJoin = ring.channel;
    select(ring.workspace_id, (navigate) => navigate({ kind: "channel", id: ring.channel }));
  }, [join, select, session.workspace_id]);
  const leave = useCallback(() => {
    api.leaveHuddle().catch(reportError);
  }, []);
  const decline = useCallback(() => {
    const ring = ringing.current;
    if (ring) api.declineHuddle(ring.workspace_id).catch(reportError);
  }, []);
  const toggleMute = useCallback(() => {
    const huddle = current.current;
    if (!huddle) return;
    api.setHuddleMuted(!huddle.muted).catch(reportError);
  }, []);

  useHuddleKeys(mine, join, leave, toggleMute);

  const value = useMemo(
    () => ({
      mine,
      isElsewhere,
      incoming,
      speaking,
      reconnecting,
      isMicDown,
      isSpeakerDown,
      join,
      leave,
      answer,
      decline,
      toggleMute,
    }),
    [
      mine,
      isElsewhere,
      incoming,
      speaking,
      reconnecting,
      isMicDown,
      isSpeakerDown,
      join,
      leave,
      answer,
      decline,
      toggleMute,
    ],
  );
  return (
    <HuddleContext.Provider value={value}>
      {children}
      {/* Replaced, not edited, for each piece of news, so the same words twice ("Muted") are said twice. */}
      <div role="status" aria-label="Huddle news" className="sr-only">
        {news && <span key={news.id}>{news.text}</span>}
      </div>
    </HuddleContext.Provider>
  );
}

const NO_PEERS: HuddlePeer[] = [];

/** Join or leave the huddle where you are, and mute, from the keyboard; the shell leaves these keys to us. */
function useHuddleKeys(
  mine: MyHuddle | null | undefined,
  join: (channel: string) => void,
  leave: () => void,
  toggleMute: () => void,
) {
  const { view, channels } = useWorkspace();
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const shortcut = SHORTCUTS.find((candidate) => candidate.matches(event));
      if (shortcut?.id === "huddleMute") {
        event.preventDefault();
        if (mine) toggleMute();
      }
      if (shortcut?.id !== "huddle") return;
      event.preventDefault();
      const here = view?.kind === "channel" ? channels.find((channel) => channel.id === view.id) : undefined;
      if (mine && (!here || mine.channel === here.id)) return leave();
      if (here && here.kind !== "notes") join(here.id);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [mine, view, channels, join, leave, toggleMute]);
}

/**
 * Who's talking. The speaking events are the truth; a load of the huddle only
 * fills in people not heard from yet, since a load that started before an
 * event can finish after it.
 */
function useSpeaking(mine: MyHuddle | null, me: string): ReadonlySet<string> {
  const channel = mine?.channel ?? null;
  // What was heard belongs to the huddle it was heard in, so a new one starts with nobody heard.
  const [heard, setHeard] = useState<{ channel: string | null; speaking: ReadonlyMap<string, boolean> }>({
    channel: null,
    speaking: new Map(),
  });
  const currentChannel = useRef(channel);
  currentChannel.current = channel;
  useEffect(
    () =>
      onAnyWorkspaceChange((_, change) => {
        if (change.kind !== "huddle_speaking") return;
        setHeard((previous) => {
          const here = currentChannel.current;
          const speaking = previous.channel === here ? new Map(previous.speaking) : new Map<string, boolean>();
          return { channel: here, speaking: speaking.set(change.member, change.speaking) };
        });
      }),
    [],
  );
  return useMemo(() => {
    const speaking = new Set<string>();
    if (!mine) return speaking;
    const heardHere = heard.channel === mine.channel ? heard.speaking : new Map<string, boolean>();
    for (const peer of mine.peers) {
      if (heardHere.get(peer.member) ?? peer.speaking) speaking.add(peer.member);
    }
    if (!mine.muted && heardHere.get(me)) speaking.add(me);
    return speaking;
  }, [mine, heard, me]);
}

/** Peers reconnecting for longer than the grace; shorter blips never show. */
function useLongReconnects(peers: HuddlePeer[]): ReadonlySet<string> {
  const [long, setLong] = useState<ReadonlySet<string>>(new Set());
  const timers = useRef(new Map<string, number>());
  const reconnectingNow = peers
    .filter((peer) => peer.link === "reconnecting")
    .map((peer) => peer.member)
    .join(" ");
  useEffect(() => {
    const now = new Set(reconnectingNow.split(" ").filter(Boolean));
    for (const [member, timer] of timers.current) {
      if (now.has(member)) continue;
      clearTimeout(timer);
      timers.current.delete(member);
    }
    for (const member of now) {
      if (timers.current.has(member)) continue;
      const timer = window.setTimeout(() => setLong((previous) => new Set(previous).add(member)), RECONNECT_GRACE_MS);
      timers.current.set(member, timer);
    }
    setLong((previous) => {
      const kept = [...previous].filter((member) => now.has(member));
      return kept.length === previous.size ? previous : new Set(kept);
    });
  }, [reconnectingNow]);
  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const timer of pending.values()) clearTimeout(timer);
    };
  }, []);
  return long;
}

function isRingChange(change: Change): boolean {
  return change.kind === "huddle_ringing" || change.kind === "huddle_ring_stopped";
}

/** True once `isOn` has held for `ms`; false again as soon as it stops. */
function useLasting(isOn: boolean, ms: number): boolean {
  const [hasLasted, setLasted] = useState(false);
  useEffect(() => {
    if (!isOn) {
      setLasted(false);
      return;
    }
    const timer = window.setTimeout(() => setLasted(true), ms);
    return () => clearTimeout(timer);
  }, [isOn, ms]);
  return isOn && hasLasted;
}

/** Says what changed in your huddle. */
function useNews(
  mine: MyHuddle | null | undefined,
  nameOf: (member: string) => string,
  placeOf: (channel: string) => string,
  say: (text: string) => void,
) {
  const previous = useRef<MyHuddle | null | undefined>(undefined);
  useEffect(() => {
    const before = previous.current;
    previous.current = mine;
    // The first load is where things already stand, not news.
    if (before === undefined || mine === undefined) return;
    const said = huddleNews(before, mine, nameOf, placeOf);
    if (said.length > 0) say(said.join(". "));
  }, [mine, nameOf, placeOf, say]);
}
