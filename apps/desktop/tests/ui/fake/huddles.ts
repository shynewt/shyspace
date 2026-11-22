/**
 * Huddles in the fake: rosters, your own call, and helpers that act as the
 * other members and the audio layer would. No audio or beacons; those are
 * covered by the Rust tests.
 */

import type { InvokeArgs } from "@tauri-apps/api/core";
import type {
  AudioDeviceChoice,
  AudioDevices,
  Change,
  Huddle,
  HuddleLink,
  Member,
  MicState,
  MyHuddle,
  OutputState,
} from "../../../src/lib/api";
import { argument, requireString } from "./helpers";
import type { PrivateView } from "./private";

/** Returned by `answer` for commands that aren't about huddles. */
export const NOT_HUDDLES = Symbol("not a huddles command");

/** Matches the core's cap. */
export const MAX_HUDDLE_PARTICIPANTS = 8;

export type HuddlesContext = {
  me: () => string;
  members: () => Member[];
  notify: (change: Change) => void;
  privateView: (channel: string) => PrivateView | null;
};

type Someone = { camera: boolean; screen: boolean; needsUpdate: boolean };

const DEVICES: AudioDevices = {
  inputs: [
    { id: "built-in-mic", name: "Built-in Microphone", is_default: true, is_bluetooth: false },
    { id: "usb-mic", name: "USB Microphone", is_default: false, is_bluetooth: false },
    { id: "headset-mic", name: "Headset", is_default: false, is_bluetooth: true },
  ],
  outputs: [
    { id: "built-in-speakers", name: "Built-in Speakers", is_default: true, is_bluetooth: false },
    { id: "headset", name: "Headset", is_default: false, is_bluetooth: true },
  ],
};

export class FakeHuddles {
  private readonly context: HuddlesContext;
  /** Others in each channel's huddle, by member id. */
  private rosters = new Map<string, Map<string, Someone>>();
  private mine: MyHuddle | null = null;
  private releaseJoin: (() => void) | null = null;
  /** Keeps `join_huddle` in "joining" until `connect()`, so tests can see that state. */
  holdJoining = false;
  /** Keeps an unmuted microphone "opening" until `releaseMic()`. */
  holdMicOpening = false;
  /** Makes the next `join_huddle` fail with this message. */
  failNextJoin: string | null = null;
  /** The `muted` argument of each `join_huddle`, in order. */
  readonly joinedMuted: boolean[] = [];
  /** The last `set_huddle_devices` choice. */
  chosenDevices: AudioDeviceChoice = { input: null, output: null };

  constructor(context: HuddlesContext) {
    this.context = context;
  }

  // ---- Acting as other members and the audio layer, for tests ------------

  /** `member` joins `channel`'s huddle, leaving any other, as their app would. */
  join(member: string, channel: string): void {
    this.leaveAny(member);
    const roster = this.rosterOf(channel);
    const isStart = roster.size === 0 && this.mine?.channel !== channel;
    roster.set(member, { camera: false, screen: false, needsUpdate: false });
    if (this.mine?.channel === channel) {
      this.mine.peers.push({ member, link: "connecting", speaking: false });
      this.context.notify({ kind: "my_huddle" });
    }
    this.context.notify({ kind: "huddle", channel });
    if (isStart && this.isMyDm(channel)) {
      const memberName = this.context.members().find((candidate) => candidate.id === member)?.name ?? "";
      this.context.notify({ kind: "huddle_started", channel, member, member_name: memberName });
    }
  }

  leave(member: string, channel: string): void {
    if (!this.rosterOf(channel).delete(member)) return;
    if (this.mine?.channel === channel) {
      this.mine.peers = this.mine.peers.filter((peer) => peer.member !== member);
      this.context.notify({ kind: "my_huddle" });
    }
    this.context.notify({ kind: "huddle", channel });
  }

  /** Others join `channel` until it holds as many as the core allows. */
  fill(channel: string): void {
    const inCall = () => this.rosterOf(channel).size + (this.mine?.channel === channel ? 1 : 0);
    for (const member of this.context.members()) {
      if (inCall() >= MAX_HUDDLE_PARTICIPANTS) return;
      if (member.id !== this.context.me() && !this.rosterOf(channel).has(member.id)) this.join(member.id, channel);
    }
    if (inCall() < MAX_HUDDLE_PARTICIPANTS) throw new Error("fake backend: not enough members to fill a huddle");
  }

  /** `member` starts or stops speaking; your own member id for your voice. */
  speaking(member: string, speaking: boolean): void {
    if (!this.mine) throw new Error("fake backend: you aren't in a huddle");
    const peer = this.mine.peers.find((candidate) => candidate.member === member);
    if (peer) peer.speaking = speaking;
    else if (member !== this.context.me()) throw new Error("fake backend: they aren't in your huddle");
    this.context.notify({ kind: "huddle_speaking", member, speaking });
  }

  link(member: string, link: HuddleLink): void {
    const peer = this.mine?.peers.find((candidate) => candidate.member === member);
    if (!peer) throw new Error("fake backend: they aren't in your huddle");
    peer.link = link;
    this.context.notify({ kind: "my_huddle" });
  }

  mic(state: MicState): void {
    this.myHuddle().mic = state;
    this.context.notify({ kind: "my_huddle" });
  }

  output(state: OutputState): void {
    this.myHuddle().output = state;
    this.context.notify({ kind: "my_huddle" });
  }

  needsUpdate(member: string, needsUpdate: boolean): void {
    for (const [channel, roster] of this.rosters) {
      const someone = roster.get(member);
      if (!someone) continue;
      someone.needsUpdate = needsUpdate;
      this.context.notify({ kind: "huddle", channel });
    }
  }

  /** Finishes a join held by `holdJoining`. */
  connect(): void {
    if (!this.releaseJoin) throw new Error("fake backend: no join is waiting");
    this.releaseJoin();
  }

  /** Finishes a microphone held "opening" by `holdMicOpening`. */
  releaseMic(): void {
    if (this.mine?.mic !== "opening") throw new Error("fake backend: the microphone isn't opening");
    this.mic("live");
  }

  // ---- The command surface ----------------------------------------------

  answer(command: string, args: InvokeArgs | undefined): unknown {
    switch (command) {
      case "huddle":
        return this.huddle(requireString(args, "channel"));
      case "my_huddle":
        return this.mine;
      case "join_huddle":
        return this.joinMine(requireString(args, "channel"), argument(args, "muted") === true);
      case "leave_huddle":
        this.leaveMine();
        return null;
      case "set_huddle_muted":
        this.setMuted(argument(args, "muted") === true);
        return null;
      case "huddle_devices":
        return DEVICES;
      case "set_huddle_devices": {
        const choice = argument(args, "choice");
        if (!isDeviceChoice(choice)) throw new Error("fake backend: missing device choice");
        this.chosenDevices = choice;
        return null;
      }
      default:
        return NOT_HUDDLES;
    }
  }

  private huddle(channel: string): Huddle {
    const others = [...this.rosterOf(channel)].map(([member, someone]) => ({
      member,
      camera: someone.camera,
      screen: someone.screen,
      is_me: false,
      needs_update: someone.needsUpdate,
    }));
    const me = { member: this.context.me(), camera: false, screen: false, is_me: true, needs_update: false };
    const participants = this.mine?.channel === channel ? [me, ...others] : others;
    return { channel, participants, is_full: participants.length >= MAX_HUDDLE_PARTICIPANTS };
  }

  private joinMine(channel: string, muted: boolean): Promise<null> | null {
    this.joinedMuted.push(muted);
    if (this.failNextJoin !== null) {
      const message = this.failNextJoin;
      this.failNextJoin = null;
      throw new Error(message);
    }
    this.leaveMine();
    if (this.rosterOf(channel).size >= MAX_HUDDLE_PARTICIPANTS) throw new Error("this huddle is full");
    this.mine = {
      channel,
      phase: "joining",
      muted,
      mic: muted ? "off" : "opening",
      output: "playing",
      peers: [...this.rosterOf(channel).keys()].map((member) => ({ member, link: "connecting", speaking: false })),
    };
    this.context.notify({ kind: "my_huddle" });
    this.context.notify({ kind: "huddle", channel });
    if (!this.holdJoining) {
      this.finishJoin();
      return null;
    }
    return new Promise((resolve) => {
      this.releaseJoin = () => {
        this.releaseJoin = null;
        this.finishJoin();
        resolve(null);
      };
    });
  }

  private finishJoin(): void {
    const mine = this.myHuddle();
    mine.phase = "in";
    if (!mine.muted && !this.holdMicOpening) mine.mic = "live";
    this.context.notify({ kind: "my_huddle" });
  }

  private leaveMine(): void {
    if (!this.mine) return;
    const channel = this.mine.channel;
    this.mine = null;
    this.releaseJoin = null;
    this.context.notify({ kind: "my_huddle" });
    this.context.notify({ kind: "huddle", channel });
  }

  private setMuted(muted: boolean): void {
    const mine = this.myHuddle();
    mine.muted = muted;
    mine.mic = muted ? "off" : this.holdMicOpening ? "opening" : "live";
    this.context.notify({ kind: "my_huddle" });
  }

  private myHuddle(): MyHuddle {
    if (!this.mine) throw new Error("you aren't in a huddle");
    return this.mine;
  }

  private rosterOf(channel: string): Map<string, Someone> {
    let roster = this.rosters.get(channel);
    if (!roster) {
      roster = new Map();
      this.rosters.set(channel, roster);
    }
    return roster;
  }

  private leaveAny(member: string): void {
    for (const [channel, roster] of this.rosters) {
      if (roster.has(member)) this.leave(member, channel);
    }
  }

  private isMyDm(channel: string): boolean {
    return this.context.privateView(channel)?.kind === "dm";
  }
}

function isDeviceChoice(value: unknown): value is AudioDeviceChoice {
  if (typeof value !== "object" || value === null || !("input" in value) || !("output" in value)) return false;
  const isDevice = (device: unknown) => device === null || typeof device === "string";
  return isDevice(value.input) && isDevice(value.output);
}
