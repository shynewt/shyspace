import type { Member, MemberStatus } from "../../lib/api";
import { formatStatusEnd } from "../../lib/people";
import { Tooltip } from "../../ui";

/** "🌴 On holiday · until Fri 18:00". */
export function describeStatus(status: MemberStatus): string {
  const words = [status.emoji, status.text].filter(Boolean).join(" ");
  return status.until_ms === null ? words : `${words} · until ${formatStatusEnd(status.until_ms)}`;
}

/** How quiet their alerts are, when not "all": "Busy" or "Mentions only". */
export function alertsLabel(status: MemberStatus): string | null {
  if (status.alerts === "nothing") return "Busy";
  if (status.alerts === "mentions") return "Mentions only";
  return null;
}

/** A person's status as it shows by their name; agents have none of their own. */
export function shownStatus(member: Member): MemberStatus | null {
  return member.is_bot ? null : member.status;
}

/**
 * The status emoji beside a name, telling the whole status on hover or
 * keyboard focus (so it's a button, though pressing it does nothing more).
 * A status without an emoji shows nothing here: the text alone would crowd
 * the name.
 */
export function StatusEmoji({ member }: { member: Member }) {
  const status = shownStatus(member);
  if (!status?.emoji) return null;
  const description = describeStatus(status);
  return (
    <Tooltip label={description}>
      <button
        type="button"
        aria-label={`Status: ${description}`}
        className="shrink-0 cursor-default rounded text-sm leading-none"
      >
        {status.emoji}
      </button>
    </Tooltip>
  );
}
