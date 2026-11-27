import type { InvokeArgs } from "@tauri-apps/api/core";
import type {
  Change,
  CheckAfterRestart,
  ConfirmedLink,
  DeviceInfo,
  DeviceNotice,
  LinkCheck,
  LinkCheckShown,
  LinkCodeSummary,
  Session,
} from "../../../src/lib/api";
import { randomHex, requireString } from "./helpers";

export const NOT_DEVICES = Symbol("not a devices command");

/** The number the fake new device shows, and the decoys beside it. */
const REAL_CODE = "482 917";
const DECOYS = ["305 116", "771 482"];
const MAX_DEVICES = 5;

/** The core's words for each refusal. */
const READING_ONLY = "This device can read now. It can send once your workspace adds it.";
const FIRST_FROM_LINKED = "Remove this one from that device, or ask your admin.";
const ADMINS_DEVICE = "The admin's own device can't be removed.";
const KEEP_ONE = "Keep at least one of your devices.";
const ALREADY_REMOVING = "Removing already. It finishes when your workspace confirms.";
const TOO_MANY = "you already have 5 devices; remove one to link another";
const DAILY_LIMIT = "you've linked several devices today; try again tomorrow";
const ALREADY_LINKING = "This device is already linking to a workspace. Pick that link up, or stop it, first.";

type Context = {
  me: () => string;
  /** The shown workspace's id. */
  workspaceId: () => string;
  isAdmin: () => boolean;
  isReadingOnly: () => boolean;
  emitFrom: (workspaceId: string, change: Change) => void;
  /** Tells the UI the device list or notices changed. */
  onChange: () => void;
  /** Opens the workspace for reading on this device, as a hand-over does. */
  startReading: () => Session;
  /** A stop while reading: the workspace goes, or, if the device was added just before, stays joined. */
  stopReading: (hasJoined: boolean) => void;
};

type StoredDevice = Omit<DeviceInfo, "may_remove" | "why_not_removable">;

/**
 * Your devices and links: the device you use making a code and picking the
 * new device's number, and this device linking to a person. Tests drive the
 * other side with `newDeviceAsks`, `handOver` and friends, and refuse as
 * the core does.
 */
export class FakeDevices {
  private devices: StoredDevice[] = [];
  private notices: DeviceNotice[] = [];
  /** Notices in workspaces that aren't shown, by workspace id. */
  private noticesIn = new Map<string, DeviceNotice[]>();
  private openCode: { code: string; hasAsked: boolean } | null = null;
  /** A link runs on this device already (begun here, or by `shy`). */
  hasLinkInProgress = false;
  private pendingHandover: { resolve: (session: Session) => void; reject: (error: Error) => void } | null = null;
  /** A link a restart interrupted before its hand-over. */
  interrupted: CheckAfterRestart | null = null;
  /** What each recheck answers next; then the last one again. */
  rechecks: CheckAfterRestart[] = [];
  /** The next rechecks fail while the link still waits, as a busy step does. */
  recheckFailures = 0;
  /** Why the last link ended, until the app takes it. */
  ended: string | null = null;
  /** The choices in the order they show, real one in the middle unless a test says otherwise. */
  choices: [string, string, string] = [DECOYS[0], REAL_CODE, DECOYS[1]];
  /** This device is a linked one, not the person's first. */
  isThisLinked = false;
  hasReachedDailyLimit = false;
  /** A confirmation is kept, but finishes later. */
  doesConfirmFinishLater = false;
  /** A stop while reading finds the device added just before. */
  joinsOnStop = false;
  /** How many times the app asked for the device list. */
  deviceListings = 0;
  /** What the system clipboard holds. */
  clipboard = "";
  /** How many times the app polled for the new device. */
  pendingLinkPolls = 0;
  /** The next polls for the new device fail, as storage out of reach does. */
  pendingLinkFailures = 0;
  readonly context: Context;

  constructor(context: Context) {
    this.context = context;
  }

  // ---- Test controls ----

  /** The person's devices: this one first, then `others`. */
  setDevices(others: Partial<StoredDevice>[]): void {
    this.devices = [this.thisDevice()];
    for (const other of others) this.devices.push(this.device(other));
    this.context.onChange();
  }

  setNotices(notices: DeviceNotice[]): void {
    this.notices = notices;
    this.context.onChange();
  }

  /** Notices in another open workspace, which says its members changed. */
  setNoticesIn(workspaceId: string, notices: DeviceNotice[]): void {
    this.noticesIn.set(workspaceId, notices);
    this.context.emitFrom(workspaceId, { kind: "members" });
  }

  /** The new device asked through the open code. */
  newDeviceAsks(): void {
    if (this.openCode) this.openCode.hasAsked = true;
  }

  /** The person's other device hands this one what it needs. */
  handOver(): void {
    this.interrupted = null;
    this.pendingHandover?.resolve(this.context.startReading());
    this.pendingHandover = null;
  }

  /** The person's other device stopped the link. */
  stopFromOtherDevice(): void {
    this.hasLinkInProgress = false;
    this.pendingHandover?.reject(new Error("Your other device stopped the link. Nothing was shared."));
    this.pendingHandover = null;
  }

  get hasOpenCode(): boolean {
    return this.openCode !== null;
  }

  get seenNotices(): boolean {
    return this.notices.every((notice) => notice.is_seen);
  }

  get removing(): string[] {
    return this.devices.filter((device) => device.is_removing).map((device) => device.id);
  }

  // ---- Commands ----

  answer(command: string, args: InvokeArgs | undefined): unknown {
    switch (command) {
      case "create_link_code":
        return this.createCode();
      case "pending_link":
        return this.pendingLink();
      case "confirm_link":
        return this.confirm(requireString(args, "picked"));
      case "cancel_link":
        this.openCode = null;
        return null;
      case "my_devices":
        this.deviceListings += 1;
        return this.listed();
      case "remove_my_device":
        return this.remove([requireString(args, "id")]);
      case "remove_my_device_and_its_links": {
        const id = requireString(args, "id");
        const linked = this.devices.find((device) => device.id === id)?.linked_devices ?? [];
        return this.remove([id, ...linked]);
      }
      case "device_notices":
        return this.notices;
      case "workspace_device_notices": {
        const workspaceId = requireString(args, "workspaceId");
        if (workspaceId === this.context.workspaceId()) return this.notices;
        return this.noticesIn.get(workspaceId) ?? [];
      }
      case "mark_device_notices_seen":
        this.notices = this.notices.map((notice) => ({ ...notice, is_seen: true }));
        this.context.onChange();
        return null;
      case "read_link_code":
        return this.readLinkCode(requireString(args, "code"));
      case "begin_device_link":
        return this.begin(requireString(args, "code"));
      case "wait_for_device_link_handover":
        return new Promise<Session>((resolve, reject) => {
          this.pendingHandover = { resolve, reject };
        });
      case "waiting_device_link":
        return this.interrupted;
      case "recheck_device_link":
        if (this.recheckFailures > 0) {
          this.recheckFailures -= 1;
          throw "This device's link is busy with another step. Wait for it, or stop the link.";
        }
        this.interrupted = this.rechecks.shift() ?? this.interrupted;
        return this.interrupted;
      case "take_ended_device_link": {
        const ended = this.ended;
        this.ended = null;
        return ended;
      }
      case "stop_device_link":
        return this.stop();
      case "clipboard_text":
        return this.clipboard;
      default:
        return NOT_DEVICES;
    }
  }

  private createCode(): string {
    if (this.devices.length >= MAX_DEVICES) throw TOO_MANY;
    if (this.hasReachedDailyLimit) throw DAILY_LIMIT;
    this.openCode = { code: `shyl1_${randomHex(40)}`, hasAsked: false };
    return this.openCode.code;
  }

  private pendingLink(): LinkCheckShown | null {
    this.pendingLinkPolls += 1;
    if (this.pendingLinkFailures > 0) {
      this.pendingLinkFailures -= 1;
      throw "can't reach the workspace's storage";
    }
    if (!this.openCode?.hasAsked) return null;
    return { choices: this.choices, platform: "Linux" };
  }

  private confirm(picked: string): ConfirmedLink {
    if (!this.openCode?.hasAsked) throw "there's no new device waiting to be confirmed";
    this.openCode = null;
    if (picked !== REAL_CODE) throw "That wasn't the number. Start again with a new code.";
    const id = randomHex(32);
    this.devices.push(this.device({ id, is_linking: true, platform: "Android", linked_ms: Date.now() }));
    this.context.onChange();
    if (this.doesConfirmFinishLater) return { device: null, is_finishing: true };
    return { device: id, is_finishing: false };
  }

  private begin(code: string): LinkCheck {
    this.readLinkCode(code);
    if (this.hasLinkInProgress) throw ALREADY_LINKING;
    this.hasLinkInProgress = true;
    return { code: REAL_CODE };
  }

  private stop(): null {
    this.interrupted = null;
    this.hasLinkInProgress = false;
    if (this.pendingHandover) {
      this.pendingHandover.reject(new Error("You stopped linking this device. Nothing was kept."));
      this.pendingHandover = null;
    } else if (this.context.isReadingOnly()) {
      this.context.stopReading(this.joinsOnStop);
    }
    return null;
  }

  private listed(): DeviceInfo[] {
    if (this.devices.length === 0) this.devices = [this.thisDevice()];
    return this.devices.map((device) => {
      const whyNot = this.whyNotRemovable(device);
      return { ...device, may_remove: whyNot === null, why_not_removable: whyNot };
    });
  }

  /** The core's rules, for one device on its own. */
  private whyNotRemovable(device: StoredDevice): string | null {
    if (device.is_removing) return ALREADY_REMOVING;
    if (this.context.isReadingOnly()) return READING_ONLY;
    if (device.is_first_device && this.isThisLinked) return FIRST_FROM_LINKED;
    if (device.is_first_device && this.context.isAdmin()) return ADMINS_DEVICE;
    const members = this.devices.filter((candidate) => !candidate.is_linking);
    if (!device.is_linking && members.length === 1) return KEEP_ONE;
    return null;
  }

  private remove(ids: string[]): null {
    for (const device of this.listed()) {
      if (ids.includes(device.id) && device.why_not_removable && !device.is_removing) {
        throw device.why_not_removable;
      }
    }
    this.devices = this.devices.flatMap((device) => {
      if (!ids.includes(device.id)) return [device];
      return device.is_linking ? [] : [{ ...device, is_removing: true }];
    });
    this.context.onChange();
    return null;
  }

  private readLinkCode(code: string): LinkCodeSummary {
    const compact = code.replace(/\s+/g, "");
    if (!compact.startsWith("shyl1_") || compact.length < 20) {
      throw "This isn't a link code. Copy it again from your other device.";
    }
    if (compact.includes("expired")) throw "This code has run out. Show a new one on your other device.";
    return { workspace_name: "Acme" };
  }

  private thisDevice(): StoredDevice {
    return this.device({
      id: this.context.me(),
      is_this_device: true,
      is_first_device: !this.isThisLinked,
      platform: "Linux",
    });
  }

  private device(fields: Partial<StoredDevice>): StoredDevice {
    return {
      id: randomHex(32),
      is_this_device: false,
      is_first_device: false,
      platform: null,
      linked_ms: null,
      is_active_now: false,
      last_active_ms: null,
      has_no_activity_seen: false,
      is_removing: false,
      is_linking: false,
      linked_devices: [],
      ...fields,
    };
  }
}
