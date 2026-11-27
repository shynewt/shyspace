import type { InvokeArgs } from "@tauri-apps/api/core";
import type {
  Change,
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

type Context = {
  me: () => string;
  /** The shown workspace's id. */
  workspaceId: () => string;
  emitFrom: (workspaceId: string, change: Change) => void;
  /** Tells the UI the device list or notices changed. */
  onChange: () => void;
  /** Opens the workspace for reading on this device, as a hand-over does. */
  startReading: () => Session;
};

/**
 * Your devices and links: the device you use making a code and picking the
 * new device's number, and this device linking to a person. Tests drive the
 * other side with `newDeviceAsks`, `handOver` and friends.
 */
export class FakeDevices {
  private devices: DeviceInfo[] = [];
  private notices: DeviceNotice[] = [];
  /** Notices in workspaces that aren't shown, by workspace id. */
  private noticesIn = new Map<string, DeviceNotice[]>();
  private openCode: { code: string; hasAsked: boolean } | null = null;
  /** A link a restart interrupted before its hand-over. */
  interrupted: LinkCheck | null = null;
  private pendingHandover: { resolve: (session: Session) => void; reject: (error: Error) => void } | null = null;
  /** The choices in the order they show, real one in the middle unless a test says otherwise. */
  choices: [string, string, string] = [DECOYS[0], REAL_CODE, DECOYS[1]];
  readonly context: Context;

  constructor(context: Context) {
    this.context = context;
  }

  // ---- Test controls ----

  /** The person's devices: this one first, then `others`. */
  setDevices(others: Partial<DeviceInfo>[]): void {
    this.devices = [this.device({ id: this.context.me(), is_this_device: true, is_first_device: true })];
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
    this.pendingHandover?.reject(new Error("Your other device stopped the link. Nothing was shared."));
    this.pendingHandover = null;
  }

  get hasOpenCode(): boolean {
    return this.openCode !== null;
  }

  get seenNotices(): boolean {
    return this.notices.every((notice) => notice.is_seen);
  }

  // ---- Commands ----

  answer(command: string, args: InvokeArgs | undefined): unknown {
    switch (command) {
      case "create_link_code":
        this.openCode = { code: `shyl1_${randomHex(40)}`, hasAsked: false };
        return this.openCode.code;
      case "pending_link":
        return this.pendingLink();
      case "confirm_link":
        return this.confirm(requireString(args, "picked"));
      case "cancel_link":
        this.openCode = null;
        return null;
      case "my_devices":
        if (this.devices.length === 0) {
          this.devices = [this.device({ id: this.context.me(), is_this_device: true, is_first_device: true })];
        }
        return this.devices;
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
        this.readLinkCode(requireString(args, "code"));
        return { code: REAL_CODE } satisfies LinkCheck;
      case "wait_for_device_link_handover":
        return new Promise<Session>((resolve, reject) => {
          this.pendingHandover = { resolve, reject };
        });
      case "waiting_device_link":
        return this.interrupted;
      case "stop_device_link":
        this.interrupted = null;
        this.pendingHandover?.reject(new Error("You stopped linking this device. Nothing was kept."));
        this.pendingHandover = null;
        return null;
      default:
        return NOT_DEVICES;
    }
  }

  private pendingLink(): LinkCheckShown | null {
    if (!this.openCode?.hasAsked) return null;
    return { choices: this.choices, platform: "Linux" };
  }

  private confirm(picked: string): string {
    if (!this.openCode?.hasAsked) throw new Error("there's no new device waiting to be confirmed");
    this.openCode = null;
    if (picked !== REAL_CODE) throw new Error("That wasn't the number. Start again with a new code.");
    const id = randomHex(32);
    this.devices.push(this.device({ id, is_linking: true, platform: "Linux", linked_ms: Date.now() }));
    this.context.onChange();
    return id;
  }

  private remove(ids: string[]): null {
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
      throw new Error("This isn't a link code. Copy it again from your other device.");
    }
    if (compact.includes("expired")) throw new Error("This code has run out. Show a new one on your other device.");
    return { workspace_name: "Acme" };
  }

  private device(fields: Partial<DeviceInfo>): DeviceInfo {
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
