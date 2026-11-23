import type { Invite, Member } from "../../../src/lib/api";
import { fakeSafetyCode, type Kind, noDetails, randomHex } from "./helpers";

const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/** Invites and join requests, as the admin sees them. */
export class FakeInvites {
  private invites: Invite[] = [];
  private readonly onChange: () => void;

  constructor(onChange: () => void) {
    this.onChange = onChange;
  }

  list(): Invite[] {
    return this.invites;
  }

  create(label: string): Invite {
    const now = Date.now();
    const invite: Invite = {
      token: randomHex(16),
      label,
      created_ms: now,
      expires_ms: now + WEEK_MS,
      state: "open",
      requester_id: null,
      requester_name: null,
      requester_is_bot: false,
      requester_safety_code: null,
    };
    this.invites.push(invite);
    this.onChange();
    return invite;
  }

  /** Someone used an open invite (or a new one) to ask to join. */
  receiveRequest(name: string, { kind }: Kind): string {
    const invite = this.invites.find((candidate) => candidate.state === "open") ?? this.create("");
    invite.state = "requested";
    invite.requester_id = randomHex(32);
    invite.requester_name = name;
    invite.requester_is_bot = kind === "bot";
    invite.requester_safety_code = "1234 5678 9012 3456 7890";
    this.onChange();
    return invite.token;
  }

  /** Accepts the request and returns the new member. */
  approve(token: string): Member {
    const invite = this.find(token);
    if (invite.state !== "requested" || !invite.requester_id) throw "this invite has no request to approve";
    invite.state = "accepted";
    this.onChange();
    return {
      id: invite.requester_id,
      name: invite.requester_name ?? "",
      is_bot: invite.requester_is_bot,
      is_admin: false,
      active: true,
      status: null,
      presence: "unreachable",
      details: noDetails(),
      picture: null,
      safety_code: fakeSafetyCode(invite.requester_id),
    };
  }

  setState(token: string, state: "rejected" | "revoked"): void {
    this.find(token).state = state;
    this.onChange();
  }

  private find(token: string): Invite {
    const invite = this.invites.find((candidate) => candidate.token === token);
    if (!invite) throw new Error(`fake backend: no invite ${token}`);
    return invite;
  }
}
